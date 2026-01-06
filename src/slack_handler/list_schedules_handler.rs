use crate::{
    db::ScheduledTaskRepository,
    errors::AppError,
};

pub async fn handle_list_schedules_command(
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
) -> Result<Vec<String>, AppError> {
    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let schedules = tasks
        .into_iter()
        .map(|t| {
            format!(
                "## {}\nUpdate {} on {}\nNext schedule: {}",
                t.channel_name, t.user_group_handle, t.cron, t.next_update_time
            )
        })
        .collect();

    Ok(schedules)
}
