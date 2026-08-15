pub mod list_schedules_handler;
pub mod new_schedule_handler;
pub mod new_schedule_wizard_handler;
pub mod setup_pagerduty_handler;
pub mod slack_request;

use list_schedules_handler::handle_list_schedules_command;
use new_schedule_handler::handle_schedule_command;
use new_schedule_wizard_handler::handle_new_schedule_wizard;
use setup_pagerduty_handler::handle_setup_pagerduty_command;
use slack_request::{Command, SlackCommandRequest, normalize_slack_user_id, parse_slack_command, parse_slack_request};
use std::env;
use std::sync::Arc;
use tracing::{Instrument, info_span};

use crate::service::slack::{send_slack_message, send_slack_view};
use crate::slack_handler::utils::slack_response::{error_section, markdown_section};
use crate::utils::logging::json_tracing;
use crate::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    config::Config,
    db::dynamodb::{ScheduledTasksDynamodb, SlackInstallationsDynamoDb},
    errors::AppError,
};
use aws_lambda_events::event::apigw::ApiGatewayProxyRequest;

pub async fn handle_slack_command_async(config: &Arc<Config>, event: ApiGatewayProxyRequest) -> Result<(), AppError> {
    json_tracing::debug!("Processing command asynchronously", event);

    let request_body = event.body.as_deref().unwrap_or("");
    let params = parse_slack_request(event.headers, request_body, &config).await?;
    let response_url = params.response_url.clone();

    let span = info_span!("dispatch_slack_command", user_name = %params.user_name);
    match dispatch_slack_command(config, params).instrument(span).await {
        Ok(()) => Ok(()),
        Err(err) => {
            json_tracing::error!("Failed to process Slack command", err = &err.to_string());
            send_slack_message(&response_url, error_section(&format!("Failed to process command. Details: {}", err)))
                .await?;
            // Return Ok so Lambda does not retry after the user was already notified.
            Ok(())
        }
    }
}

async fn dispatch_slack_command(config: &Arc<Config>, mut params: SlackCommandRequest) -> Result<(), AppError> {
    let arg = parse_slack_command(&params.command, &params.text).await?;

    if let Some(as_user) = resolve_as_user(&params.user_id, arg.as_user.as_deref(), &config.admin_user_slack_ids)? {
        if !matches!(arg.command, Some(Command::ListSchedules(_)) | None) {
            return Err(AppError::InvalidData("--as-user is only supported when listing schedules".to_string()));
        }
        json_tracing::info!("Running command as another user", as_user, caller = &params.user_id);
        params.user_id = as_user;
    }

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
        Some(Command::New) => {
            let encryptor = config.build_encryptor().await?;
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor);

            handle_new_schedule_wizard(&params, &params.trigger_id, &slack_installations_db).await?;
        }
        Some(Command::ListSchedules(_)) | _ => {
            let (page, page_size) = match &arg.command {
                Some(Command::ListSchedules(args)) => (args.page, args.page_size),
                None => (None, 5), // Defaults: page 0, 5 items per page
                _ => unreachable!(),
            };

            let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
            let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
            let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);
            let next_trigger_timestamp = scheduler
                .get_current_schedule()
                .await?
                .and_then(|s| s.next_scheduled_timestamp_utc);

            let is_admin = config.admin_user_slack_ids.contains(&params.user_id);
            let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);
            let view = handle_list_schedules_command(
                &scheduled_tasks_db,
                page,
                page_size,
                params.user_id,
                params.channel_id,
                next_trigger_timestamp,
                is_admin,
            )
            .await?;

            send_slack_view(&response_url, view).await?;
        }
    };

    Ok(())
}

/// Resolve `--as-user`. Admins only; the ID is not looked up in Slack.
fn resolve_as_user(
    caller_user_id: &str,
    as_user: Option<&str>,
    admin_user_slack_ids: &[String],
) -> Result<Option<String>, AppError> {
    let Some(raw) = as_user.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    if !admin_user_slack_ids.iter().any(|id| id == caller_user_id) {
        return Err(AppError::Unauthorized("--as-user is only available to admins".to_string()));
    }

    Ok(Some(normalize_slack_user_id(raw)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_as_user_admin_only() {
        let admins = vec!["U_ADMIN".to_string()];

        assert_eq!(resolve_as_user("U_ADMIN", None, &admins).unwrap(), None);
        assert_eq!(
            resolve_as_user("U_ADMIN", Some("U0123FAKE"), &admins).unwrap(),
            Some("U0123FAKE".to_string())
        );
        assert_eq!(
            resolve_as_user("U_ADMIN", Some("<@U0123FAKE|alice>"), &admins).unwrap(),
            Some("U0123FAKE".to_string())
        );
        assert!(resolve_as_user("U_OTHER", Some("U0123FAKE"), &admins).is_err());
    }
}
