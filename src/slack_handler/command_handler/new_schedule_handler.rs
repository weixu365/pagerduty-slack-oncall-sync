use crate::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    db::{ScheduledTaskRepository, SlackInstallationRepository},
    errors::AppError,
    service::schedule::{CreateScheduleRequest, create_new_schedule, parse_user_group},
    slack_handler::command_handler::slack_request::{ScheduleArgs, SlackCommandRequest},
};
use chrono_tz::Tz;
use std::str::FromStr;

pub async fn handle_schedule_command(
    params: SlackCommandRequest,
    arg: ScheduleArgs,
    slack_installations_db: &dyn SlackInstallationRepository,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    scheduler: EventBridgeScheduler,
) -> Result<Vec<String>, AppError> {
    let installation = slack_installations_db
        .get_slack_installation(&params.team_id, &params.enterprise_id)
        .await?;

    let (user_group_id, user_group_handle) = parse_user_group(&arg.user_group)?;

    let timezone = Tz::from_str(&arg.timezone.unwrap_or("UTC".to_string()))
        .map_err(|e| AppError::InvalidData(format!("Invalid timezone: {}", e)))?;

    let request = CreateScheduleRequest {
        enterprise_id: params.enterprise_id.clone(),
        enterprise_name: params.enterprise_name.clone(),
        is_enterprise_install: params.is_enterprise_install,
        team_id: params.team_id.clone(),
        team_domain: params.team_domain.clone(),
        channel_id: params.channel_id.clone(),
        channel_name: params.channel_name.clone(),
        user_group_id: user_group_id.clone(),
        user_group_handle: user_group_handle.clone(),
        pagerduty_schedule_id: arg.pagerduty_schedule.clone(),
        cron: arg.cron.clone(),
        timezone: timezone.to_string(),
        user_id: params.user_id.clone(),
        user_name: params.user_name.clone(),
        pagerduty_api_key: arg.pagerduty_api_key.clone(),
    };

    if let Err(err) = create_new_schedule(request, &installation, scheduled_tasks_db, scheduler).await {
        tracing::error!(%err, "Failed to create schedule");
        return Err(AppError::Error(format!("Failed to save schedule task\n{} {}", &params.command, &params.text)));
    }

    Ok(vec![format!(
        "Update user group: {}|{} based on pagerduty schedule: {}, at: {}",
        user_group_id, user_group_handle, &arg.pagerduty_schedule, &arg.cron
    )])
}
