use crate::{config::Config, errors::AppError, utils::cron::CronSchedule};
use aws_sdk_scheduler::{
    Client,
    operation::get_schedule::GetScheduleOutput,
    types::{FlexibleTimeWindow, Target},
};
use chrono::Utc;
use futures::{StreamExt, TryStreamExt, stream};

pub struct EventBridgeScheduler {
    pub(crate) client: Client,
    pub(crate) name_prefix: String,
    pub(crate) lambda_arn: String,
    pub(crate) lambda_role: String,
}

#[derive(Debug, Clone)]
pub struct EventBridgeSchedule {
    pub name: Option<String>,
    pub next_scheduled_timestamp_utc: Option<i64>,
    pub schedule_id: Option<String>,

    pub expression: Option<String>,
    pub expression_timezone: Option<String>,
    pub target: Option<String>,
    pub description: Option<String>,
}

impl EventBridgeScheduler {
    pub fn new(config: &Config, lambda_arn: String, lambda_role: String) -> EventBridgeScheduler {
        EventBridgeScheduler {
            client: Client::new(&config.aws_config),
            name_prefix: config.schedule_name_prefix.to_string(),
            lambda_arn,
            lambda_role,
        }
    }

    pub async fn get_current_schedules(&self) -> Result<Vec<EventBridgeSchedule>, AppError> {
        tracing::info!("Getting the next schedules in EventBridge Scheduler");

        let current_schedules: Vec<_> = self
            .list_schedules()
            .await?
            .iter()
            .map(|s| self.convert_to_schedule(s))
            .collect();

        tracing::info!(?current_schedules, "Found existing schedules");

        Ok(current_schedules)
    }

    pub async fn get_current_schedule(&self) -> Result<Option<EventBridgeSchedule>, AppError> {
        tracing::info!("Getting the next schedule in EventBridge Scheduler");

        let now = Utc::now().timestamp();
        let current_schedules = self.get_current_schedules().await?;
        
        match self.next_schedule(&current_schedules, now) {
            Some(schedule) => Ok(Some(schedule)),
            None => Ok(None),
        }
    }

    pub async fn update_next_schedule(&self, next_task_schedule: &CronSchedule) -> Result<(), AppError> {
        let now = Utc::now().timestamp();
        let current_schedules = self.get_current_schedules().await?;
        let current_schedule = self.next_schedule(&current_schedules, now);

        let mut current_schedule_timestamp = current_schedule
            .as_ref()
            .and_then(|s| s.next_scheduled_timestamp_utc)
            .unwrap_or(i64::MAX);
        tracing::info!(
            existing_scheduled_time = current_schedule_timestamp,
            next_schedule = ?next_task_schedule,
            "Updating schedule in EventBridge Scheduler",
        );

        if next_task_schedule.next_timestamp_utc < current_schedule_timestamp {
            let next_schedule = next_task_schedule.next_datetime.format("%FT%T");
            tracing::info!(%next_schedule, "Updating schedule");
            let flexible_time_window = FlexibleTimeWindow::builder()
                .mode(aws_sdk_scheduler::types::FlexibleTimeWindowMode::Off)
                .build()
                .map_err(|e| AppError::UnexpectedError(format!("Failed to build flexible time window: {}", e)))?;

            let target = Target::builder()
                .arn(&self.lambda_arn)
                .role_arn(&self.lambda_role)
                .build()
                .map_err(|e| AppError::UnexpectedError(format!("Failed to build target: {}", e)))?;

            self.client
                .create_schedule()
                .name(format!("{}{}", self.name_prefix, next_task_schedule.next_timestamp_utc))
                .description("{datetime: <readable date time using original timezone>, datetime_utc, original_cron }")
                .schedule_expression(format!("at({})", next_schedule))
                .schedule_expression_timezone(format!("{}", next_task_schedule.timezone))
                .flexible_time_window(flexible_time_window)
                .target(target)
                .send()
                .await?;
            current_schedule_timestamp = next_task_schedule.next_timestamp_utc;
        } else {
            let next_schedule_info = current_schedule
                .and_then(|s| {
                    let expression = s.expression?;
                    let timestamp = s.next_scheduled_timestamp_utc?;
                    Some(format!("{} {}", expression, timestamp))
                })
                .unwrap_or_else(|| "None".to_string());

            tracing::info!(next_schedule = next_schedule_info, "Keep the next schedule unchanged",);
        }

        // clean up schedules to keep only the earliest
        self.cleanup_schedules(current_schedules, current_schedule_timestamp)
            .await?;

        Ok(())
    }

    pub(crate) fn next_schedule(
        &self,
        schedules: &Vec<EventBridgeSchedule>,
        from: i64,
    ) -> Option<EventBridgeSchedule> {
        let mut earliest: Option<&EventBridgeSchedule> = None;
        let mut earliest_timestamp = i64::MAX;

        for schedule in schedules {
            let scheduled_timestamp = schedule.next_scheduled_timestamp_utc.unwrap_or_default();
            if scheduled_timestamp > from && scheduled_timestamp < earliest_timestamp {
                earliest = Some(schedule);
                earliest_timestamp = scheduled_timestamp;
            }
        }

        earliest.cloned()
    }

    pub(crate) fn convert_to_schedule(&self, schedule: &GetScheduleOutput) -> EventBridgeSchedule {
        let timestamp = schedule
            .name()
            .and_then(|s| s.trim_start_matches(self.name_prefix.as_str()).parse::<i64>().ok());

        EventBridgeSchedule {
            name: schedule.name().map(|s| s.to_owned()),
            next_scheduled_timestamp_utc: timestamp,
            schedule_id: None,

            expression: schedule.schedule_expression().map(|s| s.to_string()),
            expression_timezone: schedule.schedule_expression_timezone().map(|s| s.to_string()),
            target: schedule.target().map(|s| s.arn.clone()),
            description: schedule.description().map(|s| s.to_string()),
        }
    }

    pub(crate) async fn cleanup_schedules(
        &self,
        current_schedules: Vec<EventBridgeSchedule>,
        next_scheduled_timestamp_utc: i64,
    ) -> Result<(), AppError> {
        let clear_outdated_schedules_after = Utc::now().timestamp() - 300;

        for schedule in current_schedules {
            if let Some(schedule_timestamp_utc) = schedule.next_scheduled_timestamp_utc {
                if schedule_timestamp_utc > next_scheduled_timestamp_utc
                    || schedule_timestamp_utc <= clear_outdated_schedules_after
                {
                    if let Some(name) = schedule.name {
                        self.delete_schedules(&name).await?;
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) async fn delete_schedules(&self, name: &str) -> Result<(), AppError> {
        tracing::info!(name, "delete schedule");

        self.client.delete_schedule().name(name).send().await?;

        Ok(())
    }

    pub(crate) async fn list_schedules(&self) -> Result<Vec<GetScheduleOutput>, AppError> {
        tracing::info!("list schedules in aws eventbridge scheduler");

        let schedule_summaries: Vec<_> = self
            .client
            .list_schedules()
            .name_prefix(&self.name_prefix)
            .into_paginator()
            .items()
            .send()
            .collect::<Result<Vec<_>, _>>()
            .await?;

        let schedules: Vec<GetScheduleOutput> = stream::iter(schedule_summaries)
            .map(|schedule| {
                let client = self.client.clone();
                async move {
                    let name = schedule
                        .name()
                        .ok_or_else(|| AppError::UnexpectedError("Schedule name doesn't exist".to_string()))?;

                    client
                        .get_schedule()
                        .name(name)
                        .send()
                        .await
                        .map_err(|e| AppError::UnexpectedError(format!("Failed to get schedule details: {e:?}")))
                }
            })
            .buffer_unordered(5)
            .try_collect()
            .await?;

        Ok(schedules)
    }
}
