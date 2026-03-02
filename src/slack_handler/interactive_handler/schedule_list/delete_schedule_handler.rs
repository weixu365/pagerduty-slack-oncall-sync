use crate::slack_handler::morphism_patches::blocks_kit::SlackView;
use crate::slack_handler::morphism_patches::interaction_event::SlackInteractionBlockActionsEvent;
use crate::slack_handler::views::schedule_list::build_schedule_list_view;
use crate::utils::logging::json_tracing;
use crate::{
    db::ScheduledTaskRepository, errors::AppError,
    slack_handler::interactive_handler::slack_request::DeleteScheduleValue,
};
use slack_morphism::prelude::*;

pub async fn handle_delete_schedule(
    request: &SlackInteractionBlockActionsEvent,
    action: &SlackInteractionActionInfo,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    next_trigger_timestamp: Option<i64>,
    is_admin: bool,
) -> Result<SlackView, AppError> {
    json_tracing::info!("Deleting schedule", action, user = &request.user);

    let value_str = action
        .value
        .as_ref()
        .ok_or_else(|| AppError::InvalidData("Missing value in delete action".to_string()))?;

    let value: DeleteScheduleValue = serde_json::from_str(value_str.as_str())
        .map_err(|e| AppError::InvalidData(format!("Failed to parse delete value: {}", e)))?;

    let scheduled_task = scheduled_tasks_db
        .get_scheduled_task(&value.team_id, &value.enterprise_id, &value.task_id)
        .await?;
    let user_id = request
        .user
        .as_ref()
        .map(|u| &u.id.0)
        .ok_or_else(|| AppError::InvalidData("Missing user in request".to_string()))?;

    if user_id != &scheduled_task.created_by_user_id {
        return Err(AppError::Unauthorized("You are not authorized to delete this schedule".to_string()));
    }

    scheduled_tasks_db
        .delete_scheduled_task(&value.team_id, &value.enterprise_id, &value.task_id)
        .await?;

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
