pub mod delete_schedule_handler;
pub mod filter_change_handler;
pub mod page_size_change_handlers;
pub mod pagination_handler;
pub mod refresh_handlers;
pub mod slack_request;

use std::env;
use std::sync::Arc;

use delete_schedule_handler::handle_delete_schedule;
use filter_change_handler::handle_filter_change;
use page_size_change_handlers::handle_page_size_change;
use pagination_handler::handle_pagination;
use refresh_handlers::handle_refresh;
use slack_request::parse_slack_request;

use crate::aws::event_bridge_scheduler::EventBridgeScheduler;
use crate::service_provider::slack::send_slack_view;
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
    info!(payload=?event, "Processing interactive action asynchronously");

    let request_body = event.body.as_deref().unwrap_or("");
    let payload = parse_slack_request(event.headers, request_body, &config).await?;

    let encryptor = config.build_encryptor().await?;
    let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);

    info!(payload_response_url = %payload.response_url, "Parsed interactive payload");

    let response_url = payload.response_url.clone();

    let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
    let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
    let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);
    let next_trigger_timestamp = scheduler
        .get_current_schedule()
        .await?
        .and_then(|s| s.next_scheduled_timestamp_utc);

    if let Some(action) = payload.actions.first() {
        let slack_view = match action.action_id.as_str() {
            "delete_schedule" => {
                handle_delete_schedule(&payload, action, &scheduled_tasks_db, next_trigger_timestamp).await
            }
            "refresh" => handle_refresh(&payload, action, &scheduled_tasks_db, next_trigger_timestamp).await,
            "filter_select" => {
                handle_filter_change(&payload, action, &scheduled_tasks_db, next_trigger_timestamp).await
            }
            "page_size_select" => {
                handle_page_size_change(&payload, action, &scheduled_tasks_db, next_trigger_timestamp).await
            }
            "page_previous" | "page_next" => {
                handle_pagination(&payload, action, &scheduled_tasks_db, next_trigger_timestamp).await
            }
            _ => Err(AppError::InvalidData(format!("Unknown action_id: {}", action.action_id))),
        }?;
        send_slack_view(&response_url, slack_view).await?;
    }

    response(200, r#"{"status": "completed"}"#.to_string())
}
