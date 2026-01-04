use async_trait::async_trait;
use chrono::{DateTime, Utc};
use clap::Args;

use crate::{
    errors::AppError,
    utils::cron::{get_next_schedule_from, CronSchedule},
    utils::timestamp::get_timezone,
};

#[derive(Debug, Args, Clone)]
pub struct ScheduledTask {
    pub team: String,    // Partition Key
    pub task_id: String, // Sort Key

    pub next_update_timestamp_utc: i64,
    pub next_update_time: String,

    pub team_id: String,
    pub team_domain: String,
    pub channel_id: String,
    pub channel_name: String,
    pub enterprise_id: String,
    pub enterprise_name: String,
    pub is_enterprise_install: bool,

    pub user_group_id: String,
    pub user_group_handle: String,
    pub pager_duty_schedule_id: String,
    pub pager_duty_token: Option<String>,
    pub cron: String,
    pub timezone: String,

    pub created_by_user_id: String,
    pub created_by_user_name: String,
    pub created_at: String,
    pub last_updated_at: String,
}

impl ScheduledTask {
    pub fn calculate_next_schedule(&self, from_utc: &DateTime<Utc>) -> Result<CronSchedule, AppError> {
        let timezone = get_timezone(&self.timezone)?;
        let next_schedule = get_next_schedule_from(&self.cron, &from_utc.with_timezone(&timezone))?;
        Ok(next_schedule)
    }
}

#[async_trait]
pub trait ScheduledTaskRepository: Send + Sync {
    async fn save_scheduled_task(&self, task: &ScheduledTask) -> Result<(), AppError>;

    async fn update_next_schedule(&self, task: &ScheduledTask) -> Result<(), AppError>;

    async fn list_scheduled_tasks_in_workspace(
        &self,
        workspace_id: &String,
        workspace_name: &String,
    ) -> Result<(), AppError>;

    async fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, AppError>;

    async fn delete_scheduled_task(&self, team_id: &str, workspace_id: &str, task_id: &str) -> Result<(), AppError>;
}
