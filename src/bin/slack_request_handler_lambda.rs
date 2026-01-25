use std::env;
use std::sync::Arc;

use aws_lambda_events::event::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use aws_sdk_lambda::{Client as LambdaClient};
use aws_sdk_lambda::types::InvocationType;
use lambda_runtime::{service_fn, LambdaEvent, Error};
use on_call_support::service_provider::slack::{send_slack_message, send_slack_view};
use on_call_support::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    config::Config,
    db::dynamodb::{ScheduledTasksDynamodb, SlackInstallationsDynamoDb},
    errors::AppError,
    slack_handler::{
        interactive_handler::{handle_interactive_action, InteractivePayload},
        list_schedules_handler::handle_list_schedules_command,
        new_schedule_handler::handle_schedule_command,
        oauth_handler::handle_slack_oauth,
        setup_pagerduty_handler::handle_setup_pagerduty_command,
        slack_request::{Command, parse_slack_request},
        slack_response::response,
    },
    utils::lambda_client::{invoke_slack_command_async_handler, is_async_processing_requested},
    utils::logging,
};
use reqwest::header::HeaderName;
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

async fn handle_slack_command_async(config: &Arc<Config>, event: ApiGatewayProxyRequest) -> Result<ApiGatewayProxyResponse, AppError> {
    info!(?event, "Processing command asynchronously");

    let request_body = event.body.as_deref().unwrap_or("");
    let params = parse_slack_request(event.headers, request_body, &config).await?;
    let arg = parse_slack_command(&params.command, &params.text).await?;

    let response_url = params.response_url;
    let encryptor = config.build_encryptor().await?;

    match arg.command {
        Some(Command::Schedule(arg)) => {
            let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
            let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
            let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
            let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);

            let response_body = handle_schedule_command(params, arg, &slack_installations_db, &scheduled_tasks_db, scheduler)
                .await?;

            send_slack_message(&response_url, markdown_section(response_body)).await?;
        }
        Some(Command::SetupPagerduty(arg)) => {
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
            let response_body = handle_setup_pagerduty_command(params, arg, &slack_installations_db).await?;
            
            send_slack_message(&response_url, markdown_section(response_body)).await?;
        }
        Some(Command::ListSchedules(args)) => {
            let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);
            let response = handle_list_schedules_command(&scheduled_tasks_db, args.page, args.page_size).await?;

            send_slack_view(&response_url, response.slack_view).await?;
        }
        Some(Command::New) => {
            let response_body = markdown_section(vec![format!("Show wizard to add new schedule")]);
            send_slack_message(&response_url, response_body).await?;
        }
        None => {
            let response_body = markdown_section(vec![format!("default command")]);
            send_slack_message(&response_url, response_body).await?;
        }
    };

    response(200, r#"{"status": "completed"}"#.to_string())
}

async fn handle_slack_interactive_async(config: &Arc<Config>, event: ApiGatewayProxyRequest) -> Result<ApiGatewayProxyResponse, AppError> {
    info!(?event, "Processing interactive action asynchronously");

    let request_body = event.body.as_deref().unwrap_or("");
    let params = parse_slack_request(event.headers, request_body, &config).await?;
    
    let encryptor = config.build_encryptor().await?;
    let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);

    let payload_json = &params.payload
        .ok_or_else(|| AppError::InvalidData("Missing payload field".to_string()))?;

    let payload: InteractivePayload = serde_json::from_str(payload_json)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse payload JSON: {}", e)))?;

    info!(payload_response_url = %payload.response_url, response_url=%params.response_url, "Parsed interactive payload");

    let response_url = payload.response_url.clone();
    if response_url.is_empty() {
        warn!("response_url is empty, cannot send response to Slack");
        return response(400, format!("Invalid request, response_url is empty"));
    }

    let interactive_response = handle_interactive_action(payload, &scheduled_tasks_db).await?;

    send_slack_view(&response_url, interactive_response.slack_view).await?;

    response(200, r#"{"status": "completed"}"#.to_string())
}

fn markdown_section(contents: Vec<String>) -> String {
    let sections = contents
        .into_iter()
        .map(|p| format!(r#"{{"type": "section", "text": {{ "type": "mrkdwn", "text": "{}" }} }}"#, p))
        .collect::<Vec<String>>()
        .join(",\n");

    let response_payload = format!(r#"{{ "blocks": [{}] }}"#, sections);
    
    response_payload
}

async fn func(event: LambdaEvent<ApiGatewayProxyRequest>) -> Result<ApiGatewayProxyResponse, AppError> {
    let (event, _context) = event.into_parts();

    let env = env::var("ENV").unwrap_or("dev".to_string());
    info!(path=event.path, "Received Slack request");

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

        Some("/slack/command") => {
            info!("Processing Slack command");

            let is_handling_slack_command = is_async_processing_requested(event.headers);
            if !is_handling_slack_command {
                invoke_slack_command_async_handler(&config, event).await
            } else {
                handle_slack_command_async(&config, event).await
            }
        }

        Some("/slack/interactive") => {
            info!("Processing Slack interactive action");
            let is_handling_slack_command = is_async_processing_requested(event.headers);
            if !is_handling_slack_command {
                invoke_slack_command_async_handler(&config, event).await
            } else {
                handle_slack_interactive_async(&config, event).await
            }
        }

        _ => {
            warn!(request_path, "Received request for unknown path");
            response(400, format!("Invalid request"))
        }
    }
}
