use std::sync::Arc;

use chrono::Utc;

use crate::slack_handler::slack_events::SlackInteractionBlockActionsEvent;
use crate::{
    db::SlackInstallationRepository,
    errors::AppError,
    service::{pager_duty::PagerDuty, slack::update_slack_modal},
    slack_handler::views::new_schedule_modal::build_new_schedule_modal_with_oncall,
    utils::http_client::build_http_client,
};
use slack_morphism::prelude::*;

fn build_oncall_text(schedule_id: &str, users: Vec<crate::service::pager_duty::PagerDutyUser>) -> String {
    if users.is_empty() {
        return format!("ℹ️ No on-call users found for schedule {}", schedule_id);
    }

    let user_list = users
        .into_iter()
        .map(|user| format!("{} <{}>", user.name, user.email))
        .collect::<Vec<String>>()
        .join(", ");

    format!("ℹ️ Current on-call: {}", user_list)
}

pub async fn handle_pagerduty_schedule_change(
    request: &SlackInteractionBlockActionsEvent,
    action: &SlackInteractionActionInfo,
    slack_installations_db: &dyn SlackInstallationRepository,
) -> Result<(), AppError> {
    let schedule_id = action
        .selected_option
        .as_ref()
        .map(|opt| opt.value.clone())
        .ok_or_else(|| AppError::InvalidData("Missing selected PagerDuty schedule".to_string()))?;

    let enterprise_id = request.team.enterprise_id.as_deref().unwrap_or("");

    let installation = slack_installations_db
        .get_slack_installation(&request.team.id.0, enterprise_id)
        .await?;

    let pagerduty_token = installation.pager_duty_token.ok_or_else(|| {
        AppError::InvalidData(
            "PagerDuty API token not configured. Please run `/oncall setup-pagerduty --pagerduty-api-key YOUR_KEY` first.".to_string(),
        )
    })?;

    let http_client = Arc::new(build_http_client()?);
    let pager_duty = PagerDuty::new(http_client, pagerduty_token, schedule_id.clone());
    let users = pager_duty.get_on_call_users(Utc::now()).await?;

    let on_call_text = build_oncall_text(&schedule_id, users);
    let slack_view = build_new_schedule_modal_with_oncall(&on_call_text, Some(request));

    let view = request
        .view
        .as_ref()
        .ok_or_else(|| AppError::InvalidData("Missing view in request".to_string()))?;

    update_slack_modal(&view.state_params.id.0, &view.state_params.hash, &slack_view, &installation.access_token)
        .await?;

    Ok(())
}
