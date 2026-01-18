use crate::{
    db::scheduled_task::{ScheduledTask, ScheduledTaskRepository},
    encryptor::{Encryptor, XChaCha20Encryptor},
    errors::AppError,
};

use super::scheduled_tasks_dynamodb::ScheduledTasksDynamodb;
use aws_sdk_dynamodb::{Client, types::AttributeValue};
use aws_smithy_mocks::{RuleMode, mock, mock_client};
use chrono::Utc;
use std::{collections::HashMap, sync::Arc};

fn create_test_encryptor() -> Arc<dyn Encryptor + Send + Sync> {
    let key = "0123456789abcdef0123456789abcdef";
    Arc::new(XChaCha20Encryptor::from_key(key).unwrap())
}

fn create_test_task() -> ScheduledTask {
    ScheduledTask {
        team: "test_team:test_workspace".to_string(),
        task_id: "task_123".to_string(),
        next_update_timestamp_utc: 1234567890,
        next_update_time: "2024-01-15T10:00:00Z".to_string(),

        team_id: "test_team".to_string(),
        team_domain: "test.slack.com".to_string(),
        channel_id: "C123456".to_string(),
        channel_name: "oncall".to_string(),
        enterprise_id: "E123456".to_string(),
        enterprise_name: "Test Enterprise".to_string(),
        is_enterprise_install: false,

        user_group_id: "UG123456".to_string(),
        user_group_handle: "oncall-team".to_string(),
        pager_duty_schedule_id: "PD123456".to_string(),
        pager_duty_token: Some("pd_token_123".to_string()),
        cron: "0 9 * * *".to_string(),
        timezone: "America/New_York".to_string(),

        created_by_user_id: "U123456".to_string(),
        created_by_user_name: "test-user".to_string(),
        created_at: Utc::now().to_rfc3339(),
        last_updated_at: Utc::now().to_rfc3339(),
    }
}

async fn convert_task_to_map(task: &ScheduledTask, encryptor: &Arc<dyn Encryptor + Send + Sync>) -> Result<HashMap<String, AttributeValue>, AppError> {
    let encrypted_pagerduty_token = if let Some(token) = task.pager_duty_token.as_ref() {
        Some(encryptor.encrypt(token).await?)
    } else {
        None
    };

    let pagerduty_token_value: AttributeValue = match encrypted_pagerduty_token {
        None => AttributeValue::Null(true),
        Some(json) => AttributeValue::S(json),
    };

    let mut item = HashMap::new();
    item.insert("team".to_string(), AttributeValue::S(task.team.clone()));
    item.insert("task_id".to_string(), AttributeValue::S(task.task_id.clone()));
    item.insert("next_update_timestamp_utc".to_string(), AttributeValue::N(task.next_update_timestamp_utc.to_string()));
    item.insert("next_update_time".to_string(), AttributeValue::S(task.next_update_time.clone()));
    item.insert("team_id".to_string(), AttributeValue::S(task.team_id.clone()));
    item.insert("team_domain".to_string(), AttributeValue::S(task.team_domain.clone()));
    item.insert("channel_id".to_string(), AttributeValue::S(task.channel_id.clone()));
    item.insert("channel_name".to_string(), AttributeValue::S(task.channel_name.clone()));
    item.insert("enterprise_id".to_string(), AttributeValue::S(task.enterprise_id.clone()));
    item.insert("enterprise_name".to_string(), AttributeValue::S(task.enterprise_name.clone()));
    item.insert("is_enterprise_install".to_string(), AttributeValue::S(task.is_enterprise_install.to_string()));
    item.insert("user_group_id".to_string(), AttributeValue::S(task.user_group_id.clone()));
    item.insert("user_group_handle".to_string(), AttributeValue::S(task.user_group_handle.clone()));
    item.insert("pager_duty_schedule_id".to_string(), AttributeValue::S(task.pager_duty_schedule_id.clone()));
    item.insert("pager_duty_token".to_string(), pagerduty_token_value);
    item.insert("cron".to_string(), AttributeValue::S(task.cron.clone()));
    item.insert("timezone".to_string(), AttributeValue::S(task.timezone.clone()));
    item.insert("created_by_user_id".to_string(), AttributeValue::S(task.created_by_user_id.clone()));
    item.insert("created_by_user_name".to_string(), AttributeValue::S(task.created_by_user_name.clone()));
    item.insert("created_at".to_string(), AttributeValue::S(task.created_at.clone()));
    item.insert("last_updated_at".to_string(), AttributeValue::S(task.last_updated_at.clone()));

    Ok(item)
}

#[tokio::test]
async fn test_save_scheduled_task_with_token() -> Result<(), AppError> {
    let task = create_test_task();
    let encryptor = create_test_encryptor();

    let put_item_rule =
        mock!(Client::put_item).then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&put_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    db.save_scheduled_task(&task).await?;

    Ok(())
}

#[tokio::test]
async fn test_save_scheduled_task_without_token() -> Result<(), AppError> {
    let mut task = create_test_task();
    task.pager_duty_token = None;
    let encryptor = create_test_encryptor();

    let put_item_rule =
        mock!(Client::put_item).then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&put_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    db.save_scheduled_task(&task).await?;

    Ok(())
}

#[tokio::test]
async fn test_save_scheduled_task_validates_request() -> Result<(), AppError> {
    let task = create_test_task();
    let encryptor = create_test_encryptor();

    let expected_team = task.team.clone();
    let expected_task_id = task.task_id.clone();
    let expected_team_id = task.team_id.clone();

    let put_item_rule = mock!(Client::put_item)
        .match_requests(move |req| {
            let table = req.table_name().unwrap();
            assert_eq!(table, "test-schedules");

            let items = req.item().unwrap();
            assert_eq!(items.get("team").unwrap(), &AttributeValue::S(expected_team.clone()));
            assert_eq!(items.get("task_id").unwrap(), &AttributeValue::S(expected_task_id.clone()));
            assert_eq!(items.get("team_id").unwrap(), &AttributeValue::S(expected_team_id.clone()));
            assert!(items.contains_key("pager_duty_token"));
            true
        })
        .then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&put_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    db.save_scheduled_task(&task).await?;

    Ok(())
}

#[tokio::test]
async fn test_update_next_schedule() -> Result<(), AppError> {
    let task = create_test_task();
    let encryptor = create_test_encryptor();

    let expected_team = task.team.clone();
    let expected_task_id = task.task_id.clone();

    let update_item_rule = mock!(Client::update_item)
        .match_requests(move |req| {
            let keys = req.key().unwrap();
            assert_eq!(keys.get("team").unwrap(), &AttributeValue::S(expected_team.clone()));
            assert_eq!(keys.get("task_id").unwrap(), &AttributeValue::S(expected_task_id.clone()));

            let update_expr = req.update_expression().unwrap();
            assert!(update_expr.contains("next_update_time"));
            assert!(update_expr.contains("next_update_timestamp_utc"));
            assert!(update_expr.contains("last_updated_at"));
            true
        })
        .then_output(|| aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&update_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    db.update_next_schedule(&task).await?;

    Ok(())
}

#[tokio::test]
async fn test_list_scheduled_tasks_empty() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let scan_rule =
        mock!(Client::scan).then_output(|| aws_sdk_dynamodb::operation::scan::ScanOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&scan_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    let tasks = db.list_scheduled_tasks().await?;
    assert_eq!(tasks.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_list_scheduled_tasks_with_items() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();
    let task = create_test_task();
    let item = convert_task_to_map(&task, &encryptor).await?;

    let scan_rule = mock!(Client::scan).then_output(move || {
        aws_sdk_dynamodb::operation::scan::ScanOutput::builder()
            .set_items(Some(vec![item.clone()]))
            .build()
    });

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&scan_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    let tasks = db.list_scheduled_tasks().await?;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task_id, "task_123");
    assert_eq!(tasks[0].team_id, "test_team");
    assert_eq!(tasks[0].pager_duty_token, Some("pd_token_123".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_delete_scheduled_task() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let delete_item_rule = mock!(Client::delete_item)
        .match_requests(|req| {
            let keys = req.key().unwrap();
            assert_eq!(keys.get("team").unwrap(), &AttributeValue::S("test_team:test_workspace".to_string()));
            assert_eq!(keys.get("task_id").unwrap(), &AttributeValue::S("task_123".to_string()));

            let table = req.table_name().unwrap();
            assert_eq!(table, "test-schedules");
            true
        })
        .then_output(|| aws_sdk_dynamodb::operation::delete_item::DeleteItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&delete_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    db.delete_scheduled_task("test_team", "test_workspace", "task_123")
        .await?;

    Ok(())
}

#[tokio::test]
async fn test_team_formatting() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let put_item_rule =
        mock!(Client::put_item).then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&put_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    let team = db.team("team123", "workspace456");
    assert_eq!(team, "team123:workspace456");

    Ok(())
}

#[tokio::test]
async fn test_parse_scheduled_task_with_valid_data() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let put_item_rule =
        mock!(Client::put_item).then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&put_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor: encryptor.clone(),
    };

    let task = create_test_task();
    let item = convert_task_to_map(&task, &encryptor).await?;

    let result = db.parse_scheduled_task(&item).await?;
    assert_eq!(result.task_id, "task_123");
    assert_eq!(result.team_id, "test_team");
    assert_eq!(result.pager_duty_token, Some("pd_token_123".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_parse_scheduled_task_empty_pagerduty_token() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let put_item_rule =
        mock!(Client::put_item).then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&put_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor: encryptor.clone(),
    };

    let task = create_test_task();
    let mut item = convert_task_to_map(&task, &encryptor).await?;
    item.insert("pager_duty_token".to_string(), AttributeValue::S("".to_string()));
    
    let result = db.parse_scheduled_task(&item).await?;
    assert_eq!(result.pager_duty_token, None);

    Ok(())
}

#[tokio::test]
async fn test_parse_scheduled_task_invalid_timestamp() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let put_item_rule =
        mock!(Client::put_item).then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&put_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    let mut item = HashMap::new();
    item.insert("team".to_string(), AttributeValue::S("test_team".to_string()));
    item.insert("task_id".to_string(), AttributeValue::S("task_123".to_string()));
    item.insert("next_update_timestamp_utc".to_string(), AttributeValue::S("invalid".to_string()));
    item.insert("next_update_time".to_string(), AttributeValue::S("2024-01-15T10:00:00Z".to_string()));

    let result = db.parse_scheduled_task(&item).await;
    assert!(result.is_err());
    match result {
        Err(AppError::InvalidData(msg)) => {
            assert!(msg.contains("Invalid next_update_timestamp_utc"));
        }
        _ => panic!("Expected InvalidData error"),
    }

    Ok(())
}

#[tokio::test]
async fn test_parse_scheduled_task_missing_field() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let put_item_rule =
        mock!(Client::put_item).then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build());

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&put_item_rule]);

    let db = ScheduledTasksDynamodb {
        client,
        table_name: "test-schedules".to_string(),
        encryptor,
    };

    let mut item = HashMap::new();
    item.insert("team".to_string(), AttributeValue::S("test_team".to_string()));

    let result = db.parse_scheduled_task(&item).await;
    assert!(result.is_err());
    match result {
        Err(AppError::UnexpectedError(msg)) => {
            assert!(msg.contains("Missing or invalid field"));
        }
        _ => panic!("Expected UnexpectedError"),
    }

    Ok(())
}
