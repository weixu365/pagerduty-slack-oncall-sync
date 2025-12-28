use aws_config::SdkConfig;
use aws_sdk_dynamodb::{Client, types::AttributeValue};

use crate::{errors::AppError, encryptor::{Encryptor, EncryptedData}};
use crate::db::dynamodb_client::{get_attribute, get_optional_attribute};

use super::scheduled_task::ScheduledTask;

pub struct ScheduledTasksDynamodb {
    client: Client,
    table_name: String,
    encryptor: Encryptor,
}

impl ScheduledTasksDynamodb {
    pub fn new(config: &SdkConfig, table_name: String, encryptor: Encryptor) -> ScheduledTasksDynamodb {
        ScheduledTasksDynamodb{ client: Client::new(&config), table_name, encryptor }
    }
   
    fn team(&self, team_id: &str, workspace_id: &str) -> String {
        format!("{}:{}", team_id, workspace_id)
    }

    pub async fn save_scheduled_task(&self, task: &ScheduledTask) -> Result<(), AppError> {
        let t = task.clone();

        let encrypted_pagerduty_token_json = t.pager_duty_token
            .map(|token| self.encryptor.encrypt(&token).expect("Failed to encrypt PagerDuty api key"))
            .map(|encrypted| serde_json::to_string(&encrypted).unwrap())
            .unwrap_or_default()
        ;

        let builder = self.client
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
            .item("pager_duty_token", AttributeValue::S(encrypted_pagerduty_token_json))
            .item("cron", AttributeValue::S(t.cron))
            .item("timezone", AttributeValue::S(t.timezone))

            .item("created_by_user_id", AttributeValue::S(t.created_by_user_id))
            .item("created_by_user_name", AttributeValue::S(t.created_by_user_name))
            .item("created_at", AttributeValue::S(t.created_at))
            .item("last_updated_at", AttributeValue::S(t.last_updated_at))
        ;

        println!("Saving task {} with the next schedule at {}", task.task_id, task.next_update_time);
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
            .expression_attribute_values(":next_update_timestamp_utc", AttributeValue::N(t.next_update_timestamp_utc.to_string()))
            
        ;

        println!("Updating next schedule of task {} to {}", task.task_id, task.next_update_time);
        builder.send().await?;
        
        Ok(())
    }
   
    pub async fn list_scheduled_tasks_in_workspace(&self, _workspace_id: &String, _workspace_name: &String) -> Result<(), AppError> {
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

        // println!("Items in table:");
        // for item in items {
        //     println!("   {:?}", item);
        // }
            
        Ok(())
    }

    pub async fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, AppError> {
        let scan_output = self.client
            .scan()
            .table_name(&self.table_name)
            .send()
            .await?;

        let items: Vec<ScheduledTask> = scan_output.items.unwrap_or_else(Vec::new)
            .into_iter()
            .map(|item| {
                let pager_duty_token = get_optional_attribute(&item, "pager_duty_token").map(|encrypted_token_json| 
                    if encrypted_token_json.is_empty() {
                        None
                    } else {
                        let encrypted_token: EncryptedData = serde_json::from_str(&encrypted_token_json).expect("couldn't parse encrypted pagerduty token json");

                        Some(self.encryptor.decrypt(&encrypted_token).expect("failed to decrypt pagerduty token"))
                    }
                ).flatten();

                // let encrypted_pagerduty_token: EncryptedData = serde_json::from_str(&pagerduty_token_json).expect("couldn't parse encrypted pagerduty token json");
                // let pager_duty_token = self.encryptor.decrypt(&encrypted_pagerduty_token).expect("failed to decrypt pagerduty token");

                ScheduledTask {
                    team: get_attribute(&item, "team"),
                    task_id: get_attribute(&item, "task_id"),
                    next_update_timestamp_utc: get_attribute(&item, "next_update_timestamp_utc").parse::<i64>().unwrap(),
                    next_update_time: get_attribute(&item, "next_update_time"),

                    team_id: get_attribute(&item, "team_id"),
                    team_domain: get_attribute(&item, "team_domain"),
                    channel_id: get_attribute(&item, "channel_id"),
                    channel_name: get_attribute(&item, "channel_name"),
                    enterprise_id: get_attribute(&item, "enterprise_id"),
                    enterprise_name: get_attribute(&item, "enterprise_name"),
                    is_enterprise_install: get_attribute(&item, "is_enterprise_install").eq_ignore_ascii_case("true"),

                    user_group_id: get_attribute(&item, "user_group_id"),
                    user_group_handle: get_attribute(&item, "user_group_handle"),
                    pager_duty_schedule_id: get_attribute(&item, "pager_duty_schedule_id"),
                    pager_duty_token,
                    cron: get_attribute(&item, "cron"),
                    timezone: get_attribute(&item, "timezone"),

                    created_by_user_id: get_attribute(&item, "created_by_user_id"),
                    created_by_user_name: get_attribute(&item, "created_by_user_name"),
                    created_at: get_attribute(&item, "created_at"),
                    last_updated_at: get_attribute(&item, "last_updated_at"),
                }
            })
            .collect();

        Ok(items)
    }

    pub async fn delete_scheduled_task(&self, team_id: &str, workspace_id: &str, task_id: &str) -> Result<(), AppError> {
        let request = self.client
            .delete_item()
            .key("team", AttributeValue::S(self.team(team_id, workspace_id)))
            .key("task_id", AttributeValue::S(task_id.to_string()))
            .table_name(&self.table_name);

        println!("Deleting scheduled task from DynamoDB [{request:?}]...");
        request.send().await?;

        Ok(())
    }
}
