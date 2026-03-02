use crate::slack_handler::morphism_patches::blocks_kit::SlackView;
use crate::slack_handler::morphism_patches::interaction_event::SlackInteractionBlockActionsEvent;
use crate::slack_handler::views::schedule_list::build_schedule_list_view;
use crate::utils::logging::json_tracing;
use crate::{
    db::ScheduledTaskRepository, errors::AppError,
    slack_handler::interactive_handler::slack_request::PageSizeChangeValue,
};
use slack_morphism::prelude::*;

pub async fn handle_page_size_change(
    request: &SlackInteractionBlockActionsEvent,
    action: &SlackInteractionActionInfo,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    next_trigger_timestamp: Option<i64>,
    is_admin: bool,
) -> Result<SlackView, AppError> {
    json_tracing::info!("Changing page size", action);

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
    let view = build_schedule_list_view(
        &tasks,
        0,
        value.page_size,
        user_id,
        channel_id,
        &value.filter,
        next_trigger_timestamp,
        is_admin,
    );

    Ok(view)
}
