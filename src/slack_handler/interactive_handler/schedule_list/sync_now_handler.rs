use std::env;
use std::sync::Arc;

use crate::utils::logging::json_tracing;
use crate::slack_handler::morphism_patches::blocks_kit::SlackView;
use crate::slack_handler::morphism_patches::interaction_event::SlackInteractionBlockActionsEvent;
use crate::slack_handler::views::schedule_list::build_schedule_list_view;
use crate::{
    config::Config,
    db::{ScheduledTaskRepository, SlackInstallationRepository},
    errors::AppError,
    service::slack::Slack,
    slack_handler::interactive_handler::slack_request::PaginationValue,
    user_group_updater::SyncResult,
    utils::http_client::build_http_client,
};
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_lambda::types::InvocationType;
use slack_morphism::events::SlackInteractionActionContainer;
use slack_morphism::prelude::*;

pub async fn handle_sync_now(
    request: &SlackInteractionBlockActionsEvent,
    action: &SlackInteractionActionInfo,
    config: &Arc<Config>,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    slack_installations_db: &dyn SlackInstallationRepository,
    next_trigger_timestamp: Option<i64>,
) -> Result<SlackView, AppError> {
    json_tracing::info!("Triggering manual sync", action);

    let value_str = action
        .value
        .as_ref()
        .ok_or_else(|| AppError::InvalidData("Missing value in sync_now action".to_string()))?;

    let value: PaginationValue = serde_json::from_str(value_str.as_str())
        .map_err(|e| AppError::InvalidData(format!("Failed to parse sync_now value: {}", e)))?;

    let user_id = request
        .user
        .as_ref()
        .map(|u| &u.id.0)
        .ok_or_else(|| AppError::InvalidData("Missing user in request".to_string()))?;

    if !config.admin_user_slack_ids.contains(user_id) {
        return Err(AppError::Unauthorized(
            "You are not authorized to trigger a manual sync".to_string(),
        ));
    }

    let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
    let lambda_client = LambdaClient::new(&config.aws_config);
    let payload = serde_json::json!({ "manual": true }).to_string();
    let invoke_response = lambda_client
        .invoke()
        .function_name(&lambda_arn)
        .invocation_type(InvocationType::RequestResponse)
        .payload(aws_sdk_lambda::primitives::Blob::new(payload.into_bytes()))
        .send()
        .await?;

    json_tracing::info!("Manual sync completed synchronously");

    // Parse results and send DM
    let results: Vec<SyncResult> = invoke_response
        .payload()
        .and_then(|blob| serde_json::from_slice::<serde_json::Value>(blob.as_ref()).ok())
        .and_then(|v| v.get("results").and_then(|r| serde_json::from_value(r.clone()).ok()))
        .unwrap_or_default();

    if !results.is_empty() {
        if let Ok(installation) = slack_installations_db
            .get_slack_installation(
                &request.team.id.0,
                &request.team.enterprise_id.clone().unwrap_or_default(),
            )
            .await
        {
            let http_client = Arc::new(build_http_client().unwrap_or_default());
            let slack = Slack::new(http_client, installation.access_token);
            let message = build_sync_response_message(&results);
            let send_result = match &request.container {
                SlackInteractionActionContainer::Message(msg) => {
                    let channel_id = msg.channel_id.as_ref().map(|c| c.0.as_str()).unwrap_or(user_id);
                    slack.send_ephemeral_text(channel_id, user_id, &message, None).await
                }
                SlackInteractionActionContainer::View(_) => {
                    slack.send_message(user_id, &message).await
                }
                SlackInteractionActionContainer::MessageAttachment(_) => {
                    json_tracing::warn!("Unsupported message attachment container");
                    Ok(())
                }
            };
            if let Err(err) = send_result {
                json_tracing::warn!("Failed to send sync summary message", err = &err.to_string());
            }
        }
    }

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let channel_id = request.channel.as_ref().map(|c| &c.id.0);
    let view = build_schedule_list_view(
        &tasks,
        value.page,
        value.page_size,
        user_id,
        channel_id,
        &value.filter,
        next_trigger_timestamp,
        true,
    );

    Ok(view)
}

fn build_sync_response_message(results: &[SyncResult]) -> String {
    let count = results.len();
    let mut lines = vec![format!("⚡ *Manual sync complete* — {} schedule(s) processed", count)];
    lines.push(String::new());

    for r in results {
        let to_users = r.new_user_ids.iter().map(|id| format!("<@{}>", id)).collect::<Vec<_>>().join(", ");

        let line = if let Some(ref err) = r.error {
            format!("- Channel <#{}>, User Group <!subteam^{}>: Error: {}", r.channel_id, r.user_group_id, err)
        } else if r.changed {
            let from_users = r.original_user_ids.iter().map(|id| format!("<@{}>", id)).collect::<Vec<_>>().join(", ");
            format!("- Channel <#{}>, User Group <!subteam^{}>: changed from {} to {}", r.channel_id, r.user_group_id, from_users, to_users)
        } else {
            format!("- Channel <#{}>, User Group <!subteam^{}>: no changes, user(s): {}", r.channel_id, r.user_group_id, to_users)
        };
        lines.push(line);
    }

    lines.join("\n")
}
