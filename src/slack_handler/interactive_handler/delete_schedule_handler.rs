use crate::{
    db::ScheduledTaskRepository,
    errors::AppError,
    slack_handler::interactive_handler::slack_request::{BlockAction, DeleteScheduleValue, InteractivePayload},
};
use slack_morphism::prelude::*;
use crate::slack_handler::utils::block_kit::build_schedule_list_blocks;

pub async fn handle_delete_schedule(
    payload: &InteractivePayload,
    action: &BlockAction,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    next_trigger_timestamp: Option<i64>,
) -> Result<SlackView, AppError> {
    tracing::info!(action = ?action, user=?payload.user, "Deleting schedule");

    let value_str = action.value.as_ref()
        .ok_or_else(|| AppError::InvalidData("Missing value in delete action".to_string()))?;

    let value: DeleteScheduleValue = serde_json::from_str(value_str)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse delete value: {}", e)))?;

    let scheduled_task = scheduled_tasks_db.get_scheduled_task(&value.team_id, &value.enterprise_id, &value.task_id).await?;
    if payload.user.id != scheduled_task.created_by_user_id {
        return Err(AppError::Unauthorized("You are not authorized to delete this schedule".to_string()));
    }

    scheduled_tasks_db
        .delete_scheduled_task(&value.team_id, &value.enterprise_id, &value.task_id)
        .await?;

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let response = build_schedule_list_blocks(&tasks, value.page, value.page_size, &payload.user.id, &payload.channel.id, &value.filter, next_trigger_timestamp);

    Ok(response.slack_view)
}
