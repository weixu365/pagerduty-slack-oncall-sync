use crate::{
    db::SlackInstallationRepository,
    errors::AppError,
    service::slack::open_slack_modal,
    slack_handler::{
        morphism_patches::interaction_event::SlackInteractionBlockActionsEvent,
        views::new_schedule_modal::build_new_schedule_modal,
    },
};

/// Handle the "New" button click from the schedule list
pub async fn handle_new_schedule_button(
    request: &SlackInteractionBlockActionsEvent,
    slack_installations_db: &dyn SlackInstallationRepository,
) -> Result<(), AppError> {
    tracing::info!(user=?request.user, "Opening new schedule wizard from button");

    let enterprise_id = request.team.enterprise_id.as_deref().unwrap_or("");

    let installation = slack_installations_db
        .get_slack_installation(&request.team.id.0, enterprise_id)
        .await?;

    if installation.pager_duty_token.is_none() {
        return Err(AppError::InvalidData(
            "PagerDuty API token not configured. Please run `/oncall setup-pagerduty --pagerduty-api-key YOUR_KEY` first.".to_string(),
        ));
    }

    let modal = build_new_schedule_modal(None, None, None);
    let bot_access_token = &installation.access_token;
    let trigger_id = &request.trigger_id.0;

    open_slack_modal(trigger_id, &modal, bot_access_token).await?;

    Ok(())
}
