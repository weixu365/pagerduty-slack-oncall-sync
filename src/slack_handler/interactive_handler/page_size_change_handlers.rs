use crate::slack_handler::utils::block_kit::build_schedule_list_blocks;
use crate::{
    db::ScheduledTaskRepository,
    errors::AppError,
    slack_handler::interactive_handler::slack_request::{BlockAction, InteractivePayload, PageSizeChangeValue},
};
use slack_morphism::prelude::*;

pub async fn handle_page_size_change(
    payload: &InteractivePayload,
    action: &BlockAction,
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

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let response = build_schedule_list_blocks(
        &tasks,
        0,
        value.page_size,
        &payload.user.id,
        &payload.channel.id,
        &value.filter,
        next_trigger_timestamp,
    );

    Ok(response.slack_view)
}
