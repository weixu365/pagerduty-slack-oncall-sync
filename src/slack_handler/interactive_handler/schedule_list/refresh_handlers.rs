use crate::slack_handler::morphism_patches::blocks_kit::SlackView;
use crate::slack_handler::morphism_patches::interaction_event::SlackInteractionBlockActionsEvent;
use crate::slack_handler::views::schedule_list::build_schedule_list_view;
use crate::utils::logging::json_tracing;
use crate::{
    db::ScheduledTaskRepository, errors::AppError, slack_handler::interactive_handler::slack_request::PaginationValue,
};
use slack_morphism::prelude::*;

pub async fn handle_refresh(
    request: &SlackInteractionBlockActionsEvent,
    action: &SlackInteractionActionInfo,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    next_trigger_timestamp: Option<i64>,
    is_admin: bool,
) -> Result<SlackView, AppError> {
    json_tracing::info!("Refreshing page", action);

    let value_str = action
        .value
        .as_ref()
        .ok_or_else(|| AppError::InvalidData("Missing value in refresh action".to_string()))?;

    let value: PaginationValue = serde_json::from_str(value_str.as_str())
        .map_err(|e| AppError::InvalidData(format!("Failed to parse refresh value: {}", e)))?;

    let user_id = request
        .user
        .as_ref()
        .map(|u| &u.id.0)
        .ok_or_else(|| AppError::InvalidData("Missing user in request".to_string()))?;

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let channel_id = request.channel.as_ref().map(|c| &c.id.0);
    let view = build_schedule_list_view(
        &tasks,
        value.page,
        value.page_size,
        user_id,
        channel_id,
        &value.filter,
        next_trigger_timestamp,
        is_admin,
    );

    Ok(view)
}
