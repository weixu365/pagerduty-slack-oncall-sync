use aws_sdk_dynamodb::{types::AttributeValue, Client};

use crate::db::dynamodb_client::get_attribute;
use crate::{
    config::Config, db::dynamodb_client::get_optional_encrypted_attribute, encryptor::Encryptor, errors::AppError,
};

use super::scheduled_task::ScheduledTask;

pub struct ScheduledTasksDynamodb {
    client: Client,
    table_name: String,
    encryptor: Encryptor,
}

impl ScheduledTasksDynamodb {
    pub fn new(config: &Config, encryptor: Encryptor) -> ScheduledTasksDynamodb {
        ScheduledTasksDynamodb {
            client: Client::new(&config.aws_config),
            table_name: config.schedules_table_name.to_string(),
            encryptor,
        }
    }

    fn team(&self, team_id: &str, workspace_id: &str) -> String {
        format!("{}:{}", team_id, workspace_id)
    }

    pub async fn save_scheduled_task(&self, task: &ScheduledTask) -> Result<(), AppError> {
        let t = task.clone();

        let encrypted_pagerduty_token_json = t.pager_duty_token.as_deref().map(|token| -> Result<String, AppError> {
            let encrypted = self.encryptor.encrypt(token)?;
            let json = serde_json::to_string(&encrypted)
                .map_err(|e| AppError::UnexpectedError(format!("Failed to serialize encrypted PagerDuty token: {}", e)))?;
            Ok(json)
        }).transpose()?;

        let mut builder = self
            .client
            .put_item()
            .table_name(&self.table_name)
            .item("team", AttributeValue::S(t.team))
            .item("task_id", AttributeValue::S(t.task_id))
            .item("next_update_timestamp_utc", AttributeValue::N(t.next_update_timestamp_utc.to_string()))
            .item("next_update_time", AttributeValue::S(t.next_update_time))
            .item("team_id", AttributeValue::S(t.team_id))
            .item("team_domain", AttributeValue::S(t.team_domain))
            .item("channel_id", AttributeValue::S(t.channel_id))
            .item("channel_name", AttributeValue::S(t.channel_name))
            .item("enterprise_id", AttributeValue::S(t.enterprise_id))
            .item("enterprise_name", AttributeValue::S(t.enterprise_name))
            .item("is_enterprise_install", AttributeValue::S(t.is_enterprise_install.to_string()))
            .item("user_group_id", AttributeValue::S(t.user_group_id))
            .item("user_group_handle", AttributeValue::S(t.user_group_handle))
            .item("pager_duty_schedule_id", AttributeValue::S(t.pager_duty_schedule_id))
            .item("cron", AttributeValue::S(t.cron))
            .item("timezone", AttributeValue::S(t.timezone))
            .item("created_by_user_id", AttributeValue::S(t.created_by_user_id))
            .item("created_by_user_name", AttributeValue::S(t.created_by_user_name))
            .item("created_at", AttributeValue::S(t.created_at))
            .item("last_updated_at", AttributeValue::S(t.last_updated_at));

        if let Some(json) = encrypted_pagerduty_token_json {
            builder = builder.item("pager_duty_token", AttributeValue::S(json));
        }

        tracing::info!(task_id = task.task_id, next_update_time = task.next_update_time, "Saving task");
        builder.send().await?;

        Ok(())
    }

    pub async fn update_next_schedule(&self, task: &ScheduledTask) -> Result<(), AppError> {
        let t = task.clone();
        let builder = self.client
            .update_item()
            .table_name(&self.table_name)
            .key("team", AttributeValue::S(t.team))
            .key("task_id", AttributeValue::S(t.task_id))
            .update_expression("SET last_updated_at=:last_updated_at, next_update_time=:next_update_time, next_update_timestamp_utc=:next_update_timestamp_utc")
            .expression_attribute_values(":last_updated_at", AttributeValue::S(t.last_updated_at))
            .expression_attribute_values(":next_update_time", AttributeValue::S(t.next_update_time))
            .expression_attribute_values(":next_update_timestamp_utc", AttributeValue::N(t.next_update_timestamp_utc.to_string()));

        tracing::info!(
            task_id = task.task_id,
            next_update_time = task.next_update_time,
            "Updating next schedule of task"
        );
        builder.send().await?;

        Ok(())
    }

    pub async fn list_scheduled_tasks_in_workspace(
        &self,
        _workspace_id: &String,
        _workspace_name: &String,
    ) -> Result<(), AppError> {
        // let stream = self.client
        //     .query()
        //     .table_name(&self.table_name)
        //     .into_paginator()
        //     .items()
        //     .send();

        // stream
        //     .flat_map(|item| {
        //         let id = item
        //                     .get("id")
        //                     .and_then(|attr| attr.s.as_ref().map(|s| s.clone()))
        //                     .unwrap_or_default();

        //         // ScheduledTask {

        //         // }
        //     })
        //     .collect()
        //     .await?;

        Ok(())
    }

    pub async fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, AppError> {
        let all_items: Vec<_> = self
            .client
            .scan()
            .table_name(&self.table_name)
            .into_paginator()
            .items()
            .send()
            .collect::<Result<Vec<_>, _>>()
            .await?;

        tracing::debug!(count = all_items.len(), "Retrieved all scheduled task items from DynamoDB");

        let scheduled_tasks: Vec<ScheduledTask> = all_items
            .into_iter()
            .filter_map(|item| match self.parse_scheduled_task(&item) {
                Ok(task) => Some(task),
                Err(err) => {
                    tracing::error!(%err, item = ?item, "Failed to parse scheduled task, skipping");
                    None
                }
            })
            .collect();

        Ok(scheduled_tasks)
    }

    fn parse_scheduled_task(
        &self,
        item: &std::collections::HashMap<String, AttributeValue>,
    ) -> Result<ScheduledTask, AppError> {
        Ok(ScheduledTask {
            team: get_attribute(item, "team")?,
            task_id: get_attribute(item, "task_id")?,
            next_update_timestamp_utc: get_attribute(item, "next_update_timestamp_utc")?
                .parse::<i64>()
                .map_err(|e| AppError::InvalidData(format!("Invalid next_update_timestamp_utc: {}", e)))?,
            next_update_time: get_attribute(item, "next_update_time")?,

            team_id: get_attribute(item, "team_id")?,
            team_domain: get_attribute(item, "team_domain")?,
            channel_id: get_attribute(item, "channel_id")?,
            channel_name: get_attribute(item, "channel_name")?,
            enterprise_id: get_attribute(item, "enterprise_id")?,
            enterprise_name: get_attribute(item, "enterprise_name")?,
            is_enterprise_install: get_attribute(item, "is_enterprise_install")?.eq_ignore_ascii_case("true"),

            user_group_id: get_attribute(item, "user_group_id")?,
            user_group_handle: get_attribute(item, "user_group_handle")?,
            pager_duty_schedule_id: get_attribute(item, "pager_duty_schedule_id")?,
            pager_duty_token: get_optional_encrypted_attribute(item, "pager_duty_token", &self.encryptor)?,
            cron: get_attribute(item, "cron")?,
            timezone: get_attribute(item, "timezone")?,

            created_by_user_id: get_attribute(item, "created_by_user_id")?,
            created_by_user_name: get_attribute(item, "created_by_user_name")?,
            created_at: get_attribute(item, "created_at")?,
            last_updated_at: get_attribute(item, "last_updated_at")?,
        })
    }

    pub async fn delete_scheduled_task(
        &self,
        team_id: &str,
        workspace_id: &str,
        task_id: &str,
    ) -> Result<(), AppError> {
        let request = self
            .client
            .delete_item()
            .key("team", AttributeValue::S(self.team(team_id, workspace_id)))
            .key("task_id", AttributeValue::S(task_id.to_string()))
            .table_name(&self.table_name);

        tracing::info!(team_id, workspace_id, task_id, "Deleting scheduled task from DynamoDB");
        request.send().await?;

        Ok(())
    }
}
