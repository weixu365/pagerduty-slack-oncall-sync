use std::sync::Arc;

use crate::aws::event_bridge_scheduler::EventBridgeScheduler;
use crate::db::ScheduledTaskRepository;
use crate::service::schedule::{CreateScheduleRequest, create_new_schedule, parse_user_group};
use crate::service::slack::Slack;
use crate::slack_handler::morphism_patches::blocks_kit::SlackView;
use crate::slack_handler::morphism_patches::slack_events::SlackInteractionViewSubmissionEvent;
use crate::slack_handler::views::schedule_list::{DEFAULT_PAGE_SIZE, ScheduleFilter, build_schedule_list_blocks};
use crate::utils::http_client::build_http_client;
use crate::{db::SlackInstallationRepository, errors::AppError};

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

    // Need groups.read and channels:read scopes, so fallback to channel ID if API call fails
    let get_channel_result = slack.get_channel_by_id(&channel_id).await;
    let channel_name = match get_channel_result {
        Ok(Some(channel)) => channel.name,
        _ => channel_id.to_string(),
        // Ok(None) => return Err(AppError::InvalidData(format!("Channel not found: {}", channel_id))),
        // Err(err) => return Err(err),
    };

    Ok(channel_name)
}

async fn create_schedule(
    event: &SlackInteractionViewSubmissionEvent,
    slack_installations_db: &dyn SlackInstallationRepository,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    scheduler: EventBridgeScheduler,
) -> Result<CreateScheduleRequest, AppError> {
    let state = event
        .view
        .state_params
        .state
        .as_ref()
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

                if let Some(channel_id) = &action_state.selected_conversation {
                    return Ok(channel_id.0.clone());
                }
            }
        }
        Err(AppError::InvalidData(format!("Missing previous value for action_id: {}", action_id)))
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
    if let Err(err) =
        create_new_schedule(create_request.clone(), slack_installations_db, scheduled_tasks_db, scheduler).await
    {
        tracing::error!(%err, "Failed to create schedule");
        return Err(AppError::Error(format!("Failed to save schedule task\n{}", err)));
    }

    Ok(create_request)
}

async fn send_schedule_list(
    request: CreateScheduleRequest,
    slack_installations_db: &dyn SlackInstallationRepository,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    next_trigger_timestamp: Option<i64>,
) -> Result<(), AppError> {
    let installation = slack_installations_db
        .get_slack_installation(&request.team_id, &request.enterprise_id)
        .await?;

    let http_client = Arc::new(build_http_client()?);
    let slack = Slack::new(http_client, installation.access_token.clone());

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let response = build_schedule_list_blocks(
        &tasks,
        0,
        DEFAULT_PAGE_SIZE,
        &request.user_id,
        Some(&request.channel_id),
        &ScheduleFilter::Auto,
        next_trigger_timestamp,
    );

    let blocks = match response.slack_view {
        SlackView::Modal(modal) => modal.blocks,
        _ => return Err(AppError::InvalidData("Expected modal view".to_string())),
    };

    let blocks_json = serde_json::to_value(&blocks)
        .map_err(|e| AppError::InvalidData(format!("Failed to serialize blocks: {}", e)))?;

    let message_payload = serde_json::json!({
        "channel": request.channel_id,
        "user": request.user_id,
        "text": "📋 Scheduled Tasks",
        "blocks": blocks_json,
    });

    tracing::info!(payload=?message_payload, "Sending schedule list message to channel");
    slack.send_ephemeral_message(&message_payload).await?;

    Ok(())
}

pub async fn handle_view_submission(
    event: &SlackInteractionViewSubmissionEvent,
    slack_installations_db: &dyn SlackInstallationRepository,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    scheduler: EventBridgeScheduler,
    next_trigger_timestamp: Option<i64>,
) -> Result<(), AppError> {
    let request = create_schedule(event, slack_installations_db, scheduled_tasks_db, scheduler).await?;

    send_schedule_list(request, slack_installations_db, scheduled_tasks_db, next_trigger_timestamp).await?;

    Ok(())
}
