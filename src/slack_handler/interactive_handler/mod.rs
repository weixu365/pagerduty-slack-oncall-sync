pub mod new_schedule_modal;
pub mod schedule_list;
pub mod slack_request;
use std::env;
use std::sync::Arc;

use crate::db::SlackInstallationRepository;
use crate::slack_handler::morphism_patches::interaction_event::SlackInteractionEvent;
use crate::utils::logging::json_tracing;
use new_schedule_modal::{
    pagerduty_schedule_change_handler::handle_pagerduty_schedule_change, submission_handler::handle_view_submission,
};
use schedule_list::{
    delete_schedule_handler::handle_delete_schedule, filter_change_handler::handle_filter_change,
    new_schedule_button_handler::handle_new_schedule_button, page_size_change_handlers::handle_page_size_change,
    pagination_handler::handle_pagination, refresh_handlers::handle_refresh,
};
use slack_morphism::blocks::SlackView;
use slack_request::parse_slack_request;

use crate::aws::event_bridge_scheduler::EventBridgeScheduler;
use crate::db::dynamodb::SlackInstallationsDynamoDb;
use crate::service::slack::{send_slack_view, update_slack_view};
use crate::slack_handler::utils::request_utils::validate_request;
use crate::{
    config::Config, db::dynamodb::ScheduledTasksDynamodb, errors::AppError,
    slack_handler::utils::slack_response::response,
};
use aws_lambda_events::event::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};

pub async fn handle_slack_interactive_async(
    config: &Arc<Config>,
    event: ApiGatewayProxyRequest,
) -> Result<ApiGatewayProxyResponse, AppError> {
    json_tracing::info!("Processing slack request", event = &event);

    let request_body = event.body.as_deref().unwrap_or("");

    let secrets = config.secrets().await?;
    validate_request(event.headers, request_body, &secrets.slack_signing_secret)?;

    let slack_request = parse_slack_request(request_body)?;

    let encryptor = config.build_encryptor().await?;
    let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);
    let slack_installations_db = SlackInstallationsDynamoDb::new(&config, config.build_encryptor().await?);

    let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
    let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
    let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);
    let next_trigger_timestamp = scheduler
        .get_current_schedule()
        .await?
        .and_then(|s| s.next_scheduled_timestamp_utc);

    match slack_request {
        SlackInteractionEvent::BlockActions(block_actions_event) => {
            json_tracing::info!("Received BlockActions request", event = &block_actions_event);
            let response_url = block_actions_event.response_url.clone();

            if let Some(action) = block_actions_event.actions.as_ref().and_then(|v| v.first()) {
                let action_id = action.action_id.0.as_str();

                if action_id == "new_schedule" {
                    handle_new_schedule_button(&block_actions_event, &slack_installations_db).await?;
                    return response(200, r#"{"status": "completed"}"#.to_string());
                }

                if action_id == "pagerduty_schedule_suggestion" && block_actions_event.view.is_some() {
                    handle_pagerduty_schedule_change(&block_actions_event, action, &slack_installations_db).await?;
                    return response(200, r#"{"status": "completed"}"#.to_string());
                }

                let slack_view = match action_id {
                    "delete_schedule" => {
                        handle_delete_schedule(
                            &block_actions_event,
                            action,
                            &scheduled_tasks_db,
                            next_trigger_timestamp,
                        )
                        .await
                    }
                    "refresh" => {
                        handle_refresh(&block_actions_event, action, &scheduled_tasks_db, next_trigger_timestamp).await
                    }
                    "filter_select" => {
                        handle_filter_change(&block_actions_event, action, &scheduled_tasks_db, next_trigger_timestamp)
                            .await
                    }
                    "page_size_select" => {
                        handle_page_size_change(
                            &block_actions_event,
                            action,
                            &scheduled_tasks_db,
                            next_trigger_timestamp,
                        )
                        .await
                    }
                    "page_previous" | "page_next" => {
                        handle_pagination(&block_actions_event, action, &scheduled_tasks_db, next_trigger_timestamp)
                            .await
                    }
                    _ => Err(AppError::InvalidData(format!("Unknown action_id: {}", action_id))),
                }?;

                match response_url {
                    Some(url) => send_slack_view(url.0.as_str(), slack_view).await?,
                    None => {
                        if let Some(view_id) = &block_actions_event.view.as_ref().map(|v| v.state_params.id.clone()) {
                            let hash = block_actions_event.view.as_ref().map(|v| v.state_params.hash.clone());
                            let installation = slack_installations_db
                                .get_slack_installation(
                                    &block_actions_event.team.id.0,
                                    &block_actions_event.team.enterprise_id.unwrap_or_default(),
                                )
                                .await?;
                            update_slack_view(&view_id.0, hash, &slack_view, &installation.access_token).await?;
                        } else {
                            return Err(AppError::InvalidData(
                                "No response URL or view ID found for updating Slack view".to_string(),
                            ));
                        }
                    }
                }
            }
        }
        SlackInteractionEvent::ViewSubmission(view_submission_event) => {
            tracing::info!("Received ViewSubmission event");
            let modal_callback_id = match &view_submission_event.view.view {
                SlackView::Modal(modal_view) => modal_view.callback_id.clone(),
                _ => None,
            };

            if modal_callback_id == Some("new_schedule_form".into()) {
                handle_view_submission(
                    &view_submission_event,
                    &slack_installations_db,
                    &scheduled_tasks_db,
                    scheduler,
                    next_trigger_timestamp,
                )
                .await?;

                return response(200, r#"{"response_action":"clear"}"#.to_string());
            }
        }
        SlackInteractionEvent::ViewClosed(_) => {
            tracing::info!("Received ViewClosed event");
        }
        _ => {
            tracing::info!("Received unsupported interaction event type");
            return response(200, r#"{"status": "ignored"}"#.to_string());
        }
    }

    response(200, r#"{"status": "completed"}"#.to_string())
}
