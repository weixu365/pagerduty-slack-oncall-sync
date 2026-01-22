use std::env;
use std::sync::Arc;

use aws_lambda_events::event::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use aws_sdk_lambda::{Client as LambdaClient};
use aws_sdk_lambda::types::InvocationType;
use lambda_runtime::{service_fn, LambdaEvent, Error};
use on_call_support::{
    aws::event_bridge_scheduler::EventBridgeScheduler, config::Config, db::dynamodb::{ScheduledTasksDynamodb, SlackInstallationsDynamoDb}, errors::AppError, slack_handler::{
        list_schedules_handler::handle_list_schedules_command,
        new_schedule_handler::handle_schedule_command,
        oauth_handler::handle_slack_oauth,
        setup_pagerduty_handler::handle_setup_pagerduty_command,
        slack_request::{Command, parse_slack_request},
        slack_response::response,
    },
    utils::logging
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

async fn invoke_slack_command_async_handler(config: &Arc<Config>, event: ApiGatewayProxyRequest) -> Result<ApiGatewayProxyResponse, AppError> {
    info!("Received request from Slack, invoking lambda asynchronously");

    let lambda_client = LambdaClient::new(&config.aws_config);
    let slack_handler_arn = env::var("SLACK_HANDLER_LAMBDA_ARN")?;

    // Create a new event with async_mode flag
    let mut async_event = event.clone();
    async_event.headers.insert(
        HeaderName::from_lowercase(b"x-slack-handler-async").unwrap(),
        "true".parse().unwrap(),
    );

    let payload = serde_json::to_vec(&async_event)?;
    lambda_client
        .invoke()
        .function_name(&slack_handler_arn)
        .invocation_type(InvocationType::Event)
        .payload(aws_sdk_lambda::primitives::Blob::new(payload))
        .send()
        .await?;

    info!("Lambda invoked asynchronously, returning acknowledgment to Slack");

    response(200, r#"{"text": "Processing your request..."}"#.to_string())
}

async fn handle_slack_command_async(config: &Arc<Config>, event: ApiGatewayProxyRequest) -> Result<ApiGatewayProxyResponse, AppError> {
    info!("Processing command asynchronously");

    let encryptor = config.build_encryptor().await?;

    let request_body = event.body.as_deref().unwrap_or("");
    let (params, arg) = parse_slack_request(event.headers, request_body, &config).await?;

    let response_url = params.response_url.clone();
    if response_url.is_empty() {
        warn!("response_url is empty, cannot send response to Slack");
        return response(400, format!("Invalid request, response_url is empty"));
    }

    let response_body = match arg.command {
        Some(Command::Schedule(arg)) => {
            let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
            let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
            let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
            let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);

            handle_schedule_command(params, arg, &slack_installations_db, &scheduled_tasks_db, scheduler)
                .await?
        }
        Some(Command::SetupPagerduty(arg)) => {
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
            handle_setup_pagerduty_command(params, arg, &slack_installations_db).await?
        }
        Some(Command::ListSchedules(_args)) => {
            let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);
            handle_list_schedules_command(&scheduled_tasks_db).await?
        }
        Some(Command::New) => vec![format!("Show wizard to add new schedule")],
        None => vec![format!("default command")],
    };

    let sections = response_body
        .into_iter()
        .map(|p| format!(r#"{{"type": "section", "text": {{ "type": "mrkdwn", "text": "{}" }} }}"#, p))
        .collect::<Vec<String>>()
        .join(",\n");

    let response_payload = format!(r#"{{ "blocks": [{}] }}"#, sections);
    
    info!(response_url, "Sending response to Slack response_url");

    let client = reqwest::Client::new();
    match client
        .post(&response_url)
        .header("Content-Type", "application/json")
        .body(response_payload.clone())
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                info!("Successfully sent response to Slack");
            } else {
                error!(status = %resp.status(), "Failed to send response to Slack");
            }
        }
        Err(err) => {
            error!(%err, "Error sending response to Slack");
        }
    }

    response(200, r#"{"status": "completed"}"#.to_string())
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

            // Check if this is an async invocation (flag set when lambda invokes itself)
            let is_handling_slack_command = event.headers
                .get("x-slack-handler-async")
                .map(|v| v.to_str().unwrap_or("").to_lowercase() == "true")
                .unwrap_or(false);

            if !is_handling_slack_command {
                // First invocation from Slack - invoke lambda asynchronously and return quickly
                invoke_slack_command_async_handler(&config, event).await
            } else {
                // Second invocation (async) - process the actual command logic
                handle_slack_command_async(&config, event).await
            }
        }
        _ => {
            warn!(request_path, "Received request for unknown path");
            response(400, format!("Invalid request"))
        }
    }
}
