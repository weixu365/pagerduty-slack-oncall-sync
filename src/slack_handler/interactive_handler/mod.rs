pub mod new_schedule_modal;
pub mod schedule_list;
pub mod slack_request;
use std::env;
use std::sync::Arc;

use crate::slack_handler::slack_events::SlackInteractionEvent;
use new_schedule_modal::pagerduty_schedule_change_handler::handle_pagerduty_schedule_change;
use schedule_list::{
    delete_schedule_handler::handle_delete_schedule, filter_change_handler::handle_filter_change,
    page_size_change_handlers::handle_page_size_change, pagination_handler::handle_pagination,
    refresh_handlers::handle_refresh,
};
use slack_morphism::SlackResponseUrl;
use slack_request::parse_slack_request;

use crate::aws::event_bridge_scheduler::EventBridgeScheduler;
use crate::db::dynamodb::SlackInstallationsDynamoDb;
use crate::service::slack::send_slack_view;
use crate::slack_handler::utils::request_utils::validate_request;
use crate::{
    config::Config, db::dynamodb::ScheduledTasksDynamodb, errors::AppError,
    slack_handler::utils::slack_response::response,
};
use aws_lambda_events::event::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use tracing::info;

pub async fn handle_slack_interactive_async(
    config: &Arc<Config>,
    event: ApiGatewayProxyRequest,
) -> Result<ApiGatewayProxyResponse, AppError> {
    info!(payload=?event, "Processing slack request");

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
            info!(?block_actions_event, "Received BlockActions request");
            let response_url = block_actions_event.response_url.clone();
            info!(payload_response_url = ?block_actions_event.response_url, "Handling block_actions request");

            if let Some(action) = block_actions_event.actions.as_ref().and_then(|v| v.first()) {
                let action_id = action.action_id.0.as_str();

                match &response_url {
                    None => {
                        if action_id == "pagerduty_schedule_suggestion" && block_actions_event.view.is_some() {
                            handle_pagerduty_schedule_change(&block_actions_event, action, &slack_installations_db)
                                .await?;
                            return response(200, r#"{"status": "completed"}"#.to_string());
                        }
                    }
                    Some(SlackResponseUrl(url)) => {
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
                                handle_refresh(
                                    &block_actions_event,
                                    action,
                                    &scheduled_tasks_db,
                                    next_trigger_timestamp,
                                )
                                .await
                            }
                            "filter_select" => {
                                handle_filter_change(
                                    &block_actions_event,
                                    action,
                                    &scheduled_tasks_db,
                                    next_trigger_timestamp,
                                )
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
                                handle_pagination(
                                    &block_actions_event,
                                    action,
                                    &scheduled_tasks_db,
                                    next_trigger_timestamp,
                                )
                                .await
                            }
                            _ => Err(AppError::InvalidData(format!("Unknown action_id: {}", action_id))),
                        }?;
                        send_slack_view(url.as_str(), slack_view).await?;
                    }
                }
            }
        }
        SlackInteractionEvent::ViewSubmission(_) => {
            info!("Received ViewSubmission event");
        }
        SlackInteractionEvent::ViewClosed(_) => {
            info!("Received ViewClosed event");
        }
        _ => {
            info!("Received unsupported interaction event type");
            return response(200, r#"{"status": "ignored"}"#.to_string());
        }
    }

    response(200, r#"{"status": "completed"}"#.to_string())
}
