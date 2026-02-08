use std::sync::Arc;

use super::{
    options::{OptionItem, OptionsResponse, TextObject},
    slack_request::ExternalSelectRequest,
};
use crate::{db::SlackInstallationRepository, errors::AppError, service::pager_duty::PagerDuty};

pub async fn handle_pagerduty_schedule_options(
    request: &ExternalSelectRequest,
    slack_installations_db: &dyn SlackInstallationRepository,
    http_client: Arc<reqwest::Client>,
) -> Result<OptionsResponse, AppError> {
    tracing::info!(action_id = %request.action_id, "Fetching PagerDuty schedule options");

    let enterprise_id = request.enterprise.as_ref().map(|e| e.id.clone()).unwrap_or_default();

    let installation = slack_installations_db
        .get_slack_installation(&request.team.id, &enterprise_id)
        .await?;

    let pagerduty_token = installation.pager_duty_token.ok_or_else(|| {
        AppError::InvalidData(
            "PagerDuty API token not configured. Please run `/oncall setup-pagerduty --pagerduty-api-key YOUR_KEY` first.".to_string(),
        )
    })?;

    let pager_duty = PagerDuty::new(http_client, pagerduty_token, "".into());
    let schedules = pager_duty.list_schedules(request.value.as_deref()).await?;

    let options = schedules
        .into_iter()
        .map(|schedule| OptionItem {
            text: TextObject {
                text_type: "plain_text".to_string(),
                text: format!("{} ({})", schedule.name, schedule.id),
            },
            value: schedule.id,
        })
        .collect();

    Ok(OptionsResponse { options })
}
