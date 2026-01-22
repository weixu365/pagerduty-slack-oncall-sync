use std::env;

use aws_lambda_events::event::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
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
    info!(path=event.path, "Received Slack request");

    let config = Config::get_or_init(&env).await?;
    let secrets = config.secrets().await?;
    let encryptor = config.build_encryptor().await?;

    let request_path = &event.path;

    match request_path.as_deref() {
        Some("/slack/oauth") => {
            info!("Processing Slack OAuth callback");
            // TODO: Return error if not allowed to install app based on ENV
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
            let request_body = event.body.as_deref().unwrap_or("");
            let (params, arg) = parse_slack_request(event.headers, request_body, &config).await?;

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

            response(200, format!(r#"{{ "blocks": [{}] }}"#, sections))
        }
        _ => {
            warn!(request_path, "Received request for unknown path");
            response(400, format!("Invalid request"))
        }
    }
}
