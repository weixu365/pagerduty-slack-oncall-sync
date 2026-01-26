use crate::{
    db::ScheduledTaskRepository,
    errors::AppError,
    slack_handler::interactive_handler::slack_request::{BlockAction, InteractivePayload},
};
use slack_morphism::prelude::*;
use crate::slack_handler::utils::block_kit::{build_schedule_list_blocks, DEFAULT_PAGE_SIZE};

pub async fn handle_page_size_change(
    payload: &InteractivePayload,
    action: &BlockAction,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
) -> Result<SlackView, AppError> {
    tracing::info!(action = ?action, "Changing page size");

    let page_size: usize = action
        .selected_option
        .as_ref()
        .and_then(|opt| opt.value.parse().ok())
        .unwrap_or(DEFAULT_PAGE_SIZE);

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let response = build_schedule_list_blocks(&tasks, 0, page_size, &payload.user.id);

    Ok(response.slack_view)
}
