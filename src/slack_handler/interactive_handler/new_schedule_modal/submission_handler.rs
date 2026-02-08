use std::sync::Arc;

use crate::aws::event_bridge_scheduler::EventBridgeScheduler;
use crate::db::ScheduledTaskRepository;
use crate::service::schedule::{CreateScheduleRequest, create_new_schedule, parse_user_group};
use crate::service::slack::Slack;
use crate::slack_handler::morphism_patches::slack_events::SlackInteractionViewSubmissionEvent;
use crate::utils::http_client::build_http_client;
use crate::{
    db::SlackInstallationRepository,
    errors::AppError,
};

async fn get_channel_name(
    team_id: &str,
    enterprise_id: &str,
    slack_installations_db: &dyn SlackInstallationRepository,
    channel_id: &str,
) -> Result<String, AppError> {
    let installation = slack_installations_db
        .get_slack_installation(&team_id, &enterprise_id)
        .await?;

    let http_client = Arc::new(build_http_client()?);
    let slack = Slack::new(http_client, installation.access_token);

    let channel = slack
        .get_channel_by_id(&channel_id)
        .await?
        .ok_or_else(|| AppError::InvalidData(format!("Channel not found: {}", channel_id)))?;

    Ok(channel.name)
}

pub async fn handle_view_submission(
    event: &SlackInteractionViewSubmissionEvent,
    slack_installations_db: &dyn SlackInstallationRepository,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    scheduler: EventBridgeScheduler,
) -> Result<(), AppError> {
    let state = event.view.state_params.state.as_ref()
        .ok_or_else(|| AppError::InvalidData("Missing view state".to_string()))?;

    let get_value = |action_id: &str| -> Result<String, AppError> {
        let slack_action = action_id.into();
        for block_states in state.values.values() {
            if let Some(action_state) = block_states.get(&slack_action) {
                if let Some(selected_option) = &action_state.selected_option {
                    return Ok(selected_option.value.clone());
                }

                if let Some(value) = &action_state.value {
                    return Ok(value.clone());
                }

                if let Some(channel_id) = &action_state.selected_channel {
                    return Ok(channel_id.0.clone());
                }
            }
        }
        Err(AppError::InvalidData(format!("Missing value for action_id: {}", action_id)))
    };

    let cron = get_value("cron_value")?;
    let timezone = get_value("timezone_suggestion")?;
    let user_group_value = get_value("user_group_suggestion")?;
    let (user_group_id, user_group_handle) = parse_user_group(&user_group_value)?;
    
    let team_id = event.team.id.0.clone();
    let enterprise_id = event.team.enterprise_id.clone().unwrap_or_default();
    let channel_id = get_value("channel_value")?;
    let channel_name = get_channel_name(&team_id, &enterprise_id, slack_installations_db, &channel_id).await?;

    let create_request = CreateScheduleRequest {
        enterprise_id: enterprise_id.clone(),
        enterprise_name: event.team.enterprise_name.clone().unwrap_or_default(),
        is_enterprise_install: event.is_enterprise_install,
        team_id: team_id.clone(),
        team_domain: event.team.domain.clone().unwrap_or_default(),
        channel_id: channel_id.clone(),
        channel_name,
        user_group_id: user_group_id.clone(),
        user_group_handle,
        pagerduty_schedule_id: get_value("pagerduty_schedule_suggestion")?,
        cron: cron.clone(),
        timezone: timezone.clone(),
        user_id: event.user.id.0.clone(),
        user_name: event.user.name.clone().unwrap_or_default(),
        pagerduty_api_key: None,
    };

    // Create the schedule
    if let Err(err) = create_new_schedule(
        create_request,
        slack_installations_db,
        scheduled_tasks_db,
        scheduler
    ).await {
        tracing::error!(%err, "Failed to create schedule");
        return Err(AppError::Error(format!("Failed to save schedule task\n{}", err)));
    }

    let installation = slack_installations_db
        .get_slack_installation(&team_id, &enterprise_id)
        .await?;

    let http_client = Arc::new(build_http_client()?);
    let slack = Slack::new(http_client, installation.access_token);

    let success_message = format!(
        "✅ Schedule created successfully!\n• Channel: <#{}>\n• User Group: <@{}>\n• Cron: `{}`\n• Timezone: {}",
        channel_id,
        user_group_id,
        cron,
        timezone
    );

    if let Err(err) = slack.send_message(&channel_id, &success_message).await {
        tracing::warn!(%err, "Failed to send confirmation message, but schedule was created");
    }

    Ok(())  // Modal closes automatically
}
