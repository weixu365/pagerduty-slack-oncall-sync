use crate::utils::logging::json_tracing;
use crate::{
    db::SlackInstallationRepository,
    errors::AppError,
    service::slack::open_slack_modal,
    slack_handler::{
        command_handler::slack_request::SlackCommandRequest,
        views::new_schedule_modal::build_new_schedule_modal,
    },
};

/// Build the wizard modal for creating a new schedule
pub async fn handle_new_schedule_wizard(
    params: &SlackCommandRequest,
    trigger_id: &str,
    slack_installations_db: &dyn SlackInstallationRepository,
) -> Result<(), AppError> {
    json_tracing::info!("Opening new schedule wizard", user_id = &params.user_id);

    // Check if PagerDuty token is configured
    let installation = slack_installations_db
        .get_slack_installation(&params.team_id, &params.enterprise_id)
        .await?;

    if installation.pager_duty_token.is_none() {
        return Err(AppError::InvalidData(
            "PagerDuty API token not configured. Please run `/oncall setup-pagerduty --pagerduty-api-key YOUR_KEY` first.".to_string(),
        ));
    }

    let modal = build_new_schedule_modal(None, None, None);
    let bot_access_token = &installation.access_token;
    open_slack_modal(trigger_id, &modal, bot_access_token).await?;

    Ok(())
}
