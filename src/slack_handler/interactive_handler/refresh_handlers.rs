use crate::slack_handler::utils::block_kit::build_schedule_list_blocks;
use crate::{
    db::ScheduledTaskRepository,
    errors::AppError,
    slack_handler::interactive_handler::slack_request::{BlockAction, InteractiveRequest, PaginationValue},
};
use slack_morphism::prelude::*;

pub async fn handle_refresh(
    request: &InteractiveRequest,
    action: &BlockAction,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    next_trigger_timestamp: Option<i64>,
) -> Result<SlackView, AppError> {
    tracing::info!(action = ?action, "Refreshing page");

    let value_str = action
        .value
        .as_ref()
        .ok_or_else(|| AppError::InvalidData("Missing value in refresh action".to_string()))?;

    let value: PaginationValue = serde_json::from_str(value_str)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse refresh value: {}", e)))?;

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let response = build_schedule_list_blocks(
        &tasks,
        value.page,
        value.page_size,
        &request.user.id,
        request.channel.as_ref().map(|c| &c.id),
        &value.filter,
        next_trigger_timestamp,
    );

    Ok(response.slack_view)
}
