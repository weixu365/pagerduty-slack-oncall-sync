use chrono::{DateTime, Utc};
use clap::Args;

use crate::{cron::{get_next_schedule_from, CronSchedule}, timestamp::get_timezone};

#[derive(Debug, Args, Clone)]
pub struct ScheduledTask {
    pub team: String, // Partition Key
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
    pub fn calculate_next_schedule(&self, from_utc: &DateTime<Utc>) -> Option<CronSchedule> {
        let timezone = get_timezone(&self.timezone);
        get_next_schedule_from(&self.cron, &from_utc.with_timezone(&timezone))
    }
}
