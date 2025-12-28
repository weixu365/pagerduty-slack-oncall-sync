use crate::{errors::AppError, scheduled_tasks::ScheduledTask, secrets::SecretsClient, encryptor::Encryptor};

use super::scheduled_tasks_dynamodb::ScheduledTasksDynamodb;
use aws_config::BehaviorVersion;
use chrono::Utc;

async fn create_db() -> Result<ScheduledTasksDynamodb, AppError> {
    let config = ::aws_config::load_defaults(BehaviorVersion::latest()).await;
    let secrets_client = SecretsClient::new(&config);
    let encryption_key = secrets_client.get_secret("on-call-support/secrets").await?;
    let encryptor = Encryptor::new(&encryption_key.encryption_key);
    let db = ScheduledTasksDynamodb::new(&config, "on-call-support-schedules-dev".to_string(), encryptor);

    Ok(db)
}

#[tokio::test]
async fn save_scheduled_task_with_pagerduty_token_to_db() -> Result<(), AppError> {
    let task = ScheduledTask {
        team: "test_team_workspace".to_string(),
        task_id: "task_id".to_string(),
        next_update_timestamp_utc: Utc::now().timestamp(),
        next_update_time: Utc::now().timestamp().to_string(),

        team_id: format!("workspace_id"),
        team_domain: "team_domain".to_string(),
        channel_id: "channel_id".to_string(),
        channel_name: "channel_name".to_string(),
        enterprise_id: "enterprise_id".to_string(),
        enterprise_name: "enterprise_name".to_string(),
        is_enterprise_install: false,

        user_group_id: "user_group_id".to_string(),
        user_group_handle: "user_group_handle".to_string(),
        pager_duty_schedule_id: "pager_duty_schedule_id".to_string(),
        pager_duty_token: None,
        cron: "cron".to_string(),
        timezone: "timezone".to_string(),

        created_by_user_id: "U6HHP84N9".to_string(),
        created_by_user_name: "test-user".to_string(),
        created_at: Utc::now().to_rfc3339(),
        last_updated_at: Utc::now().to_rfc3339(),
    };
    
    let db = create_db().await?;
    db.save_scheduled_task(&task).await?;
    
    Ok(())
}

#[tokio::test]
async fn save_scheduled_task_without_pagerduty_token_to_db() -> Result<(), AppError> {
    let task = ScheduledTask {
        team: "test_team_workspace".to_string(),
        task_id: "task_id".to_string(),
        next_update_timestamp_utc: Utc::now().timestamp(),
        next_update_time: Utc::now().timestamp().to_string(),

        team_id: format!("workspace_id"),
        team_domain: "team_domain".to_string(),
        channel_id: "channel_id".to_string(),
        channel_name: "channel_name".to_string(),
        enterprise_id: "enterprise_id".to_string(),
        enterprise_name: "enterprise_name".to_string(),
        is_enterprise_install: false,

        user_group_id: "user_group_id".to_string(),
        user_group_handle: "user_group_handle".to_string(),
        pager_duty_schedule_id: "pager_duty_schedule_id".to_string(),
        pager_duty_token: Some("pager_duty_token".to_string()),
        cron: "cron".to_string(),
        timezone: "timezone".to_string(),

        created_by_user_id: "U6HHP84N9".to_string(),
        created_by_user_name: "test-user".to_string(),
        created_at: Utc::now().to_rfc3339(),
        last_updated_at: Utc::now().to_rfc3339(),
    };
    
    let db = create_db().await?;
    db.save_scheduled_task(&task).await?;
    
    Ok(())
}

#[tokio::test]
async fn list_scheduled_task_to_db() -> Result<(), AppError> {
    let db = create_db().await?;
    let tasks = db.list_scheduled_tasks().await?;

    println!("Items in table:");
    for item in tasks {
        println!("   {:?}", item);
    }
    
    Ok(())
}
