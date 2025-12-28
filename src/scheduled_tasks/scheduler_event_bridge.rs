use aws_config::SdkConfig;
use chrono::Utc;
use tokio_stream::{self as stream, StreamExt as TokioStreamExt};
use crate::{errors::AppError, cron::CronSchedule};
use aws_sdk_scheduler::{Client, types::{FlexibleTimeWindow, Target}, operation::get_schedule::GetScheduleOutput};

pub struct EventBridgeScheduler {
    client: Client,
    name_prefix: String,
    lambda_arn: String,
    lambda_role: String,
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
    pub fn new(config: &SdkConfig, name_prefix: String, lambda_arn: String, lambda_role: String) -> EventBridgeScheduler {
        EventBridgeScheduler {
            client: Client::new(&config),
            name_prefix,
            lambda_arn,
            lambda_role,
        }
    }
    
    pub async fn update_next_schedule(&self, next_task_schedule: &CronSchedule) -> Result<(), AppError> {
        println!("Updating next schedule to: {:?}", next_task_schedule);

        let mut current_schedules: Vec<_> = self.list_schedules()
            .await?
            .iter()
            .map(|s| self.convert_to_schedule(s)).collect();

        current_schedules.sort_by(|a, b| a.next_scheduled_timestamp_utc.cmp(&b.next_scheduled_timestamp_utc));

        println!("Found existing schedules: {:?}", current_schedules);
        
        let next_schedule = self.get_next_schedule(&current_schedules, next_task_schedule.next_timestamp_utc);
        let mut next_schedule_timestamp = next_schedule.as_ref().and_then(|s| s.next_scheduled_timestamp_utc).unwrap_or(i64::MAX);
        println!("Found the next schedule at time: {:?}", next_schedule_timestamp);

        if next_task_schedule.next_timestamp_utc < next_schedule_timestamp {
            println!("Updating next schedule to: {}", next_task_schedule.next_datetime.format("%FT%T"));
            self.client
                .create_schedule()
                .name(format!("{}{}", self.name_prefix, next_task_schedule.next_timestamp_utc))
                .description("{datetime: <readable date time using original timezone>, datetime_utc, original_cron }")
                .schedule_expression(format!("at({})", next_task_schedule.next_datetime.format("%FT%T")))
                .schedule_expression_timezone(format!("{}", next_task_schedule.timezone))
                .flexible_time_window(FlexibleTimeWindow::builder().mode(aws_sdk_scheduler::types::FlexibleTimeWindowMode::Off).build().unwrap())
                .target(Target::builder().arn(&self.lambda_arn).role_arn(&self.lambda_role).build().unwrap())
                .send()
                .await?;
            next_schedule_timestamp = next_task_schedule.next_timestamp_utc;
        } else {
            println!("Keep the next schedule unchanged: {}", next_schedule.map(|s| format!("{} {}", s.expression.unwrap(), s.next_scheduled_timestamp_utc.unwrap())).unwrap());
        }

        // clean up schedules to keep only the earliest
        self.cleanup_schedules(current_schedules, next_schedule_timestamp).await?;
        
        Ok(())
    }
    
    fn get_next_schedule(&self, schedules: &Vec<EventBridgeSchedule>, before: i64) -> Option<EventBridgeSchedule>
    {
        let now = Utc::now().timestamp();
        for schedule in schedules {
            let scheduled_timestamp = schedule.next_scheduled_timestamp_utc.unwrap_or_default();
            if scheduled_timestamp > now && scheduled_timestamp <= before {
                return Some(schedule.clone())
            }
        }
        
        None
    }

    fn convert_to_schedule(&self, schedule: &GetScheduleOutput) -> EventBridgeSchedule {
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

    async fn cleanup_schedules(&self, current_schedules: Vec<EventBridgeSchedule>, next_scheduled_timestamp_utc: i64) -> Result<(), AppError> {
        let clear_outdated_schedules_after = Utc::now().timestamp() - 300;

        for schedule in current_schedules {
            if let Some(schedule_timestamp_utc) = schedule.next_scheduled_timestamp_utc {
                if schedule_timestamp_utc > next_scheduled_timestamp_utc || schedule_timestamp_utc <= clear_outdated_schedules_after {
                    self.delete_schedules(&schedule.name.unwrap()).await?;
                }
            }
        }

        Ok(())
    }

    async fn delete_schedules(&self, name: &str) -> Result<(), AppError> {
        println!("delete schedule: {}", name);

        self.client
            .delete_schedule()
            .name(name)
            .send()
            .await?;

        Ok(())
    }

    async fn list_schedules(&self) -> Result<Vec<GetScheduleOutput>, AppError> {
        println!("list schedules in aws eventbridge scheduler");

        let schedule_summaries: Result<Vec<_>, _> = self.client
            .list_schedules()
            .name_prefix(&self.name_prefix)
            .into_paginator()
            .items()
            .send()
            .collect()
            .await;

        // let schedule_summaries = TokioStreamExt::collect::<Result<Vec<_>, _>>(paginator).await?;
        
        let schedules: Vec<GetScheduleOutput> = stream::iter(schedule_summaries?).then(|schedule| async move {
            let x = self.client.get_schedule()
                .name(schedule.name().expect("name doesn't exists"))
                .send()
                .await.expect("Failed to get schedule details");
            x
        }).collect().await;

        Ok(schedules)
    }
}


#[cfg(test)]
mod tests {
    use aws_config::BehaviorVersion;
    use chrono::Utc;
    use chrono_tz::Tz;
    use std::str::FromStr;

    use crate::{scheduled_tasks::{scheduler_event_bridge::EventBridgeScheduler, ScheduledTask}, errors::AppError, cron::get_next_schedule_from};

    #[tokio::test]
    async fn test_update_next_schedule() -> Result<(), AppError>{
        let config = ::aws_config::load_defaults(BehaviorVersion::latest()).await;
        let scheduler_name_prefix = "on-call-support-dev_UpdateUserGroupSchedule_";
        let lambda_arn = "arn:aws:lambda:ap-southeast-2:807579936170:function:on-call-support-dev-UpdateUserGroups";
        let lambda_role_arn = "arn:aws:iam::807579936170:role/on-call-support-dev-ap-southeast-2-lambdaRole";
        let scheduler = EventBridgeScheduler::new(&config, scheduler_name_prefix.to_string(), lambda_arn.to_string(), lambda_role_arn.to_string());

        let task = ScheduledTask {
            team: "".to_string(),
            task_id: "".to_string(),
            next_update_timestamp_utc: Utc::now().timestamp(),
            next_update_time: Utc::now().to_rfc3339().to_string(),

            team_id: "".to_string(),
            team_domain: "".to_string(),
            channel_id: "".to_string(),
            channel_name: "".to_string(),
            enterprise_id: "".to_string(),
            enterprise_name: "".to_string(),
            is_enterprise_install: false,

            user_group_id: "".to_string(),
            user_group_handle: "".to_string(),
            pager_duty_schedule_id: "".to_string(),
            pager_duty_token: None,
            cron: "0 5 ? * MON-FRI *".to_string(),
            timezone: "Australia/Melbourne".to_string(),

            created_by_user_id: "U6HHTEST".to_string(),
            created_by_user_name: "test-user".to_string(),
            created_at: Utc::now().to_rfc3339(),
            last_updated_at: Utc::now().to_rfc3339(),
        };

        let timezone = Tz::from_str(&task.timezone).unwrap();
        let from = Utc::now().with_timezone(&timezone);

        let next_schedule = get_next_schedule_from(&task.cron, &from).expect("The cron has no future scheduled time from now");
        
        scheduler.update_next_schedule(&next_schedule).await?;

        let schedules = scheduler.list_schedules().await?;
        
        for item in &schedules {
            println!("Schedule\n  - name: {:?}\n  - cron: {:?} {:?}\n  - target: {:?} {:?}\n  - flexible window mode: {:?}\n  - description: {:?}\n",
                item.name,
                item.schedule_expression.as_ref(),
                item.schedule_expression_timezone.as_ref(),
                item.target.as_ref().map(|t| t.arn()),
                item.target.as_ref().map(|t| t.role_arn()),
                item.flexible_time_window.as_ref().map(|w| w.mode()),
                item.description(),
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_list_schedules() -> Result<(), AppError> {
        let config = ::aws_config::load_defaults(BehaviorVersion::latest()).await;
        let lambda_arn = "arn:aws:lambda:ap-southeast-2:807579936170:function:on-call-support-dev-UpdateUserGroups";
        let lambda_role_arn = "arn:aws:iam::807579936170:role/on-call-support-dev-ap-southeast-2-lambdaRole";
        let scheduler = EventBridgeScheduler::new(&config, "on".to_string(), lambda_arn.to_string(), lambda_role_arn.to_string());
        let schedules = scheduler.list_schedules().await?;
        
        for item in &schedules {
            println!("Schedule\n  - name: {:?}\n  - cron: {:?} {:?}\n  - target: {:?} {:?}\n  - flexible window mode: {:?}\n  - description: {:?}\n",
                item.name,
                item.schedule_expression.as_ref(),
                item.schedule_expression_timezone.as_ref(),
                item.target.as_ref().map(|t| t.arn()),
                item.target.as_ref().map(|t| t.role_arn()),
                item.flexible_time_window.as_ref().map(|w| w.mode()),
                item.description(),
            );
        }

        Ok(())
    }
}
