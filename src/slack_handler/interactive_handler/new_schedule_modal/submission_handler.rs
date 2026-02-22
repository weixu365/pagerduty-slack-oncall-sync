use std::sync::Arc;

use crate::aws::event_bridge_scheduler::EventBridgeScheduler;
use crate::db::{SlackInstallation, ScheduledTaskRepository};
use crate::service::schedule::{CreateScheduleRequest, create_new_schedule, parse_user_group};
use crate::service::slack::{Slack, update_slack_view};
use crate::slack_handler::morphism_patches::blocks_kit::SlackView;
use crate::slack_handler::morphism_patches::interaction_event::SlackInteractionViewSubmissionEvent;
use crate::slack_handler::views::new_schedule_modal::{build_new_schedule_modal, build_success_modal};
use crate::slack_handler::views::schedule_list::{build_schedule_list_view, DEFAULT_PAGE_SIZE, ScheduleFilter};
use crate::utils::http_client::build_http_client;
use crate::utils::logging::json_tracing;
use crate::{db::SlackInstallationRepository, errors::AppError};

async fn get_channel_name(slack: &Slack, channel_id: &str) -> Result<String, AppError> {
    // Need groups.read and channels:read scopes, so fallback to channel ID if API call fails
    let channel_name = match slack.get_channel_by_id(channel_id).await {
        Ok(Some(channel)) => channel.name,
        _ => channel_id.to_string(),
    };
    Ok(channel_name)
}

async fn create_schedule(
    event: &SlackInteractionViewSubmissionEvent,
    installation: &SlackInstallation,
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

    let channel_id = get_value("channel_value")?;
    let http_client = Arc::new(build_http_client()?);
    let slack = Slack::new(http_client, installation.access_token.clone());
    let channel_name = get_channel_name(&slack, &channel_id).await?;

    let create_request = CreateScheduleRequest {
        enterprise_id: event.team.enterprise_id.clone().unwrap_or_default(),
        enterprise_name: event.team.enterprise_name.clone().unwrap_or_default(),
        is_enterprise_install: event.is_enterprise_install,
        team_id: event.team.id.0.clone(),
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

    create_new_schedule(create_request.clone(), installation, scheduled_tasks_db, scheduler).await?;

    Ok(create_request)
}

async fn send_schedule_list(
    request: &CreateScheduleRequest,
    installation: &SlackInstallation,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    next_trigger_timestamp: Option<i64>,
    is_admin: bool,
) -> Result<(), AppError> {
    let http_client = Arc::new(build_http_client()?);
    let slack = Slack::new(http_client, installation.access_token.clone());

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let view = build_schedule_list_view(
        &tasks,
        0,
        DEFAULT_PAGE_SIZE,
        &request.user_id,
        Some(&request.channel_id),
        &ScheduleFilter::Auto,
        next_trigger_timestamp,
        is_admin,
    );

    let blocks = match view {
        SlackView::Modal(modal) => modal.blocks,
        _ => return Err(AppError::InvalidData("Expected modal view".to_string())),
    };

    let blocks_json = serde_json::to_value(&blocks)
        .map_err(|e| AppError::InvalidData(format!("Failed to serialize blocks: {}", e)))?;

    json_tracing::info!(
        "Sending schedule list ephemeral message to user in channel",
        channel=&request.channel_id,
        user=&request.user_id,
        payload=&blocks_json,
    );
    slack.send_ephemeral_text(&request.channel_id, &request.user_id, "📋 Scheduled Tasks", Some(&blocks_json)).await?;

    Ok(())
}

pub async fn handle_view_submission(
    event: &SlackInteractionViewSubmissionEvent,
    slack_installations_db: &dyn SlackInstallationRepository,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    scheduler: EventBridgeScheduler,
    next_trigger_timestamp: Option<i64>,
    is_admin: bool,
) -> Result<(), AppError> {
    let installation = slack_installations_db
        .get_slack_installation(
            &event.team.id.0,
            &event.team.enterprise_id.clone().unwrap_or_default(),
        )
        .await?;

    let result = create_schedule(event, &installation, scheduled_tasks_db, scheduler).await;

    match result {
        Err(ref err) => {
            tracing::error!(error = %err, "Error creating schedule from view submission");
            let view_id = event.view.state_params.id.clone();
            let error_view = build_new_schedule_modal(None, None, Some(&err.to_string()));
            update_slack_view(&view_id.0, &error_view, &installation.access_token).await?;
        }
        Ok(ref request) => {
            send_schedule_list(request, &installation, scheduled_tasks_db, next_trigger_timestamp, is_admin).await?;
            let view_id = event.view.state_params.id.clone();
            update_slack_view(&view_id.0, &build_success_modal(), &installation.access_token).await?;
        }
    }

    Ok(())
}
