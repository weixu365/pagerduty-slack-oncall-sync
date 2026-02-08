use crate::slack_handler::morphism_patches::slack_events::SlackInteractionBlockActionsEvent;
use crate::slack_handler::views::schedule_list::build_schedule_list_blocks;
use crate::{
    db::ScheduledTaskRepository, errors::AppError,
    slack_handler::interactive_handler::slack_request::PageSizeChangeValue,
};
use slack_morphism::prelude::*;
use crate::slack_handler::morphism_patches::blocks_kit::SlackView;

pub async fn handle_page_size_change(
    request: &SlackInteractionBlockActionsEvent,
    action: &SlackInteractionActionInfo,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    next_trigger_timestamp: Option<i64>,
) -> Result<SlackView, AppError> {
    tracing::info!(action = ?action, "Changing page size");

    let value_str = action
        .selected_option
        .as_ref()
        .map(|opt| opt.value.as_str())
        .ok_or_else(|| AppError::InvalidData("Missing value in page size change action".to_string()))?;

    let value: PageSizeChangeValue = serde_json::from_str(value_str)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse page size change value: {}", e)))?;

    let user_id = request
        .user
        .as_ref()
        .map(|u| &u.id.0)
        .ok_or_else(|| AppError::InvalidData("Missing user in request".to_string()))?;

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let channel_id = request.channel.as_ref().map(|c| &c.id.0);
    let response = build_schedule_list_blocks(
        &tasks,
        0,
        value.page_size,
        user_id,
        channel_id,
        &value.filter,
        next_trigger_timestamp,
    );

    Ok(response.slack_view)
}
