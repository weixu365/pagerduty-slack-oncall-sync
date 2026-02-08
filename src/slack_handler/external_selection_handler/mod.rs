pub mod options;
pub mod pagerduty_schedule_select_handler;
pub mod slack_request;
pub mod timezone_select_handler;
pub mod user_group_select_handler;
use pagerduty_schedule_select_handler::handle_pagerduty_schedule_options;
use timezone_select_handler::handle_timezone_options;
use user_group_select_handler::handle_user_group_options;

use std::sync::Arc;

use slack_request::parse_slack_request;

use crate::db::dynamodb::SlackInstallationsDynamoDb;
use crate::slack_handler::utils::request_utils::validate_request;
use crate::utils::http_client::build_http_client;
use crate::{config::Config, errors::AppError, slack_handler::utils::slack_response::response};
use aws_lambda_events::event::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use tracing::info;

pub async fn handle_slack_external_select(
    config: &Arc<Config>,
    event: ApiGatewayProxyRequest,
) -> Result<ApiGatewayProxyResponse, AppError> {
    info!(payload=?event, "Processing external select request");
    let request_body = event.body.as_deref().unwrap_or("");

    let secrets = config.secrets().await?;
    validate_request(event.headers, request_body, &secrets.slack_signing_secret)?;

    let request = parse_slack_request(request_body)?;
    info!("Handling block_suggestion request for action: {}", request.action_id);

    match request.action_id.as_str() {
        "user_group_suggestion" => {
            let encryptor = config.build_encryptor().await?;
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor);
            let http_client = Arc::new(build_http_client()?);

            let options = handle_user_group_options(&request, &slack_installations_db, http_client.clone()).await?;

            let json_response = serde_json::to_string(&options)?;
            return response(200, json_response);
        }
        "pagerduty_schedule_suggestion" => {
            let encryptor = config.build_encryptor().await?;
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor);
            let http_client = Arc::new(build_http_client()?);

            let options =
                handle_pagerduty_schedule_options(&request, &slack_installations_db, http_client.clone()).await?;

            let json_response = serde_json::to_string(&options)?;
            return response(200, json_response);
        }
        "timezone_suggestion" => {
            let options = handle_timezone_options(&request).await?;

            let json_response = serde_json::to_string(&options)?;
            return response(200, json_response);
        }

        _ => Err(AppError::InvalidData(format!("Unknown external select action_id: {}", request.action_id))),
    }
}
