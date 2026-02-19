use std::env;

use aws_lambda_events::event::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use on_call_support::slack_handler::command_handler::handle_slack_command_async;
use on_call_support::slack_handler::events_handler::handle_slack_events;
use on_call_support::slack_handler::external_selection_handler::handle_slack_external_select;
use on_call_support::slack_handler::interactive_handler::handle_slack_interactive_async;
use on_call_support::{
    config::Config,
    db::dynamodb::SlackInstallationsDynamoDb,
    errors::AppError,
    slack_handler::{
        oauth_handler::oauth_handler::handle_slack_oauth,
        utils::slack_response::response,
        views::new_schedule_modal::build_loading_modal,
    },
    utils::lambda_client::{invoke_slack_command_async_handler, is_async_processing_requested},
    utils::logging,
};
use tokio;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Error> {
    logging::init_logging();
    info!("Start handling Slack request");

    let service_func = service_fn(func);
    let result = lambda_runtime::run(service_func).await;

    match result {
        Ok(()) => {
            info!("Lambda execution completed successfully");
            Ok(())
        }
        Err(err) => {
            error!(error = %err, "Lambda execution failed");
            Err(err)
        }
    }
}

async fn func(event: LambdaEvent<ApiGatewayProxyRequest>) -> Result<ApiGatewayProxyResponse, AppError> {
    let (event, _context) = event.into_parts();

    let env = env::var("ENV").unwrap_or("dev".to_string());
    info!(path = event.path, "Received Slack request");

    let config = Config::get_or_init(&env).await?;

    let request_path = &event.path;

    match request_path.as_deref() {
        Some("/slack/oauth") => {
            info!("Processing Slack OAuth callback");
            // TODO: Return error if not allowed to install app based on ENV

            let secrets = config.secrets().await?;
            let encryptor = config.build_encryptor().await?;
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());

            match handle_slack_oauth(slack_installations_db, &secrets, event.query_string_parameters).await {
                Ok(res) => {
                    info!("Successfully processed Slack OAuth request");
                    Ok(res)
                }
                Err(err) => {
                    error!(%err, request_path, "Failed to process Slack OAuth request");
                    Err(err.into())
                }
            }
        }

        Some("/slack/external_select") => {
            // Handle block_suggestion synchronously - Slack requires response within 3 seconds
            info!("Handling block_suggestion synchronously");
            handle_slack_external_select(&config, event).await
        }

        Some("/slack/command") => {
            info!("Processing Slack command");

            let is_handling_slack_command = is_async_processing_requested(&event.headers);
            if is_handling_slack_command {
                handle_slack_command_async(&config, event).await?;
                response(200, r#"{"status": "completed"}"#.to_string())
            } else {
                invoke_slack_command_async_handler(&config, event).await?;
                response(200, "".to_string())
            }
        }

        Some("/slack/interactive") => {
            info!("Processing Slack interactive action");

            let is_handling_slack_command = is_async_processing_requested(&event.headers);
            if is_handling_slack_command {
                handle_slack_interactive_async(&config, event).await?;
                response(200, r#"{"status": "completed"}"#.to_string())
            } else {
                let sync_ack = build_interactive_sync_ack(event.body.as_deref().unwrap_or(""));
                invoke_slack_command_async_handler(&config, event).await?;
                response(200, sync_ack)
            }
        }

        Some("/slack/events") => {
            info!("Processing Slack events");

            let request_body = event.body.as_deref().unwrap_or("");
            let is_url_verification = request_body.contains("\"type\":\"url_verification\"");

            let is_handling_slack_command = is_async_processing_requested(&event.headers);
            if is_handling_slack_command || is_url_verification {
                Ok(handle_slack_events(&config, event).await?)
            } else {
                invoke_slack_command_async_handler(&config, event).await?;
                response(200, "".to_string())
            }
        }

        _ => {
            warn!(request_path, "Received request for unknown path");
            response(400, format!("Invalid request"))
        }
    }
}

/// Returns the synchronous acknowledgment body for a Slack interactive request.
///
/// For `view_submission` events on `new_schedule_form`, returns a `response_action: update`
/// with a loading modal so the view stays open while the async lambda processes the request.
/// For all other interactive events, returns an empty body (Slack default acknowledgment).
fn build_interactive_sync_ack(body: &str) -> String {
    let is_new_schedule_submission = body.contains("view_submission") && body.contains("new_schedule_form");
    if !is_new_schedule_submission {
        return "".to_string();
    }

    let loading_modal = build_loading_modal();
    match serde_json::to_value(&loading_modal) {
        Ok(view_json) => serde_json::json!({
            "response_action": "update",
            "view": view_json,
        })
        .to_string(),
        Err(e) => {
            warn!(error = %e, "Failed to serialize loading modal, falling back to empty ack");
            "".to_string()
        }
    }
}
