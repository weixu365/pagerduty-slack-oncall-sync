pub mod list_schedules_handler;
pub mod new_schedule_handler;
pub mod new_schedule_wizard_handler;
pub mod setup_pagerduty_handler;
pub mod slack_request;

use list_schedules_handler::handle_list_schedules_command;
use new_schedule_handler::handle_schedule_command;
use new_schedule_wizard_handler::handle_new_schedule_wizard;
use setup_pagerduty_handler::handle_setup_pagerduty_command;
use slack_request::{Command, parse_slack_command, parse_slack_request};
use std::env;
use std::sync::Arc;

use crate::service::slack::{send_slack_message, send_slack_view};
use crate::slack_handler::utils::slack_response::markdown_section;
use crate::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    config::Config,
    db::dynamodb::{ScheduledTasksDynamodb, SlackInstallationsDynamoDb},
    errors::AppError,
};
use aws_lambda_events::event::apigw::ApiGatewayProxyRequest;
use tracing::info;

pub async fn handle_slack_command_async(config: &Arc<Config>, event: ApiGatewayProxyRequest) -> Result<(), AppError> {
    info!(payload=?event, "Processing command asynchronously");

    let request_body = event.body.as_deref().unwrap_or("");
    let params = parse_slack_request(event.headers, request_body, &config).await?;
    let arg = parse_slack_command(&params.command, &params.text).await?;

    let response_url = params.response_url.clone();
    let encryptor = config.build_encryptor().await?;

    match arg.command {
        Some(Command::Schedule(arg)) => {
            let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
            let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
            let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
            let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);

            let response_body =
                handle_schedule_command(params, arg, &slack_installations_db, &scheduled_tasks_db, scheduler).await?;

            send_slack_message(&response_url, markdown_section(response_body)).await?;
        }
        Some(Command::SetupPagerduty(arg)) => {
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
            let response_body = handle_setup_pagerduty_command(params, arg, &slack_installations_db).await?;

            send_slack_message(&response_url, markdown_section(response_body)).await?;
        }
        Some(Command::ListSchedules(args)) => {
            let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
            let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
            let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);
            let next_trigger_timestamp = scheduler
                .get_current_schedule()
                .await?
                .and_then(|s| s.next_scheduled_timestamp_utc);

            let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);
            let response = handle_list_schedules_command(
                &scheduled_tasks_db,
                args.page,
                args.page_size,
                params.user_id,
                params.channel_id,
                next_trigger_timestamp,
            )
            .await?;

            send_slack_view(&response_url, response.slack_view).await?;
        }
        Some(Command::New) => {
            let encryptor = config.build_encryptor().await?;
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor);

            handle_new_schedule_wizard(&params, &params.trigger_id, &slack_installations_db).await?;
        }
        None => {
            let response_body = markdown_section(vec![format!("default command")]);
            send_slack_message(&response_url, response_body).await?;
        }
    };

    Ok(())
}
