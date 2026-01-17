use std::env;

use lambda_http::{Body, Error, Request, RequestExt, Response, service_fn};
use on_call_support::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    config::Config,
    db::dynamodb::{ScheduledTasksDynamodb, SlackInstallationsDynamoDb},
    encryptor::Encryptor,
    slack_handler::{
        list_schedules_handler::handle_list_schedules_command,
        new_schedule_handler::handle_schedule_command,
        oauth_handler::handle_slack_oauth,
        setup_pagerduty_handler::handle_setup_pagerduty_command,
        slack_request::{Command, parse_slack_request},
    },
    utils::http_util::response,
    utils::logging,
};
use tokio;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Error> {
    logging::init_logging();
    info!("Start handling Slack request");

    let service_func = service_fn(func);
    let result = lambda_http::run(service_func).await;

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

async fn func(request: Request) -> Result<Response<Body>, Error> {
    let env = env::var("ENV").unwrap_or("dev".to_string());
    let config = Config::get_or_init(&env).await?;
    let secrets = config.secrets().await?;
    let encryptor = Encryptor::from_key(&secrets.encryption_key)?;

    let request_path = request.uri().path();
    let method = request.method().as_str();

    info!(method, request_path, "Received Slack request");

    match request_path {
        "/slack/oauth" => {
            info!("Processing Slack OAuth callback");
            // TODO: Return error if not allowed to install app based on ENV
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());

            match handle_slack_oauth(slack_installations_db, &secrets, request.path_parameters()).await {
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
        "/slack/command" => {
            info!("Processing Slack command");
            let (params, arg) = parse_slack_request(request, &config).await?;

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

            response(200, format!(r#"{{ "blocks": [{}] }}"#, sections)).map_err(|err| err.into())
        }
        _ => {
            warn!(method, request_path, "Received request for unknown path");
            response(400, format!("Invalid request")).map_err(|err| err.into())
        }
    }
}
