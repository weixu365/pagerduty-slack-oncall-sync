use super::event_bridge_scheduler::{EventBridgeSchedule, EventBridgeScheduler};
use crate::{errors::AppError, utils::cron::CronSchedule};
use aws_sdk_scheduler::{
    Client,
    operation::{
        delete_schedule::DeleteScheduleOutput, get_schedule::GetScheduleOutput, list_schedules::ListSchedulesOutput,
    },
    types::{FlexibleTimeWindow, FlexibleTimeWindowMode, ScheduleSummary, Target},
};
use aws_smithy_mocks::{RuleMode, mock, mock_client};
use chrono::{Datelike, TimeZone, Timelike, Utc};
use chrono_tz::America::New_York;

fn create_mock_scheduler(client: Client) -> EventBridgeScheduler {
    EventBridgeScheduler {
        client,
        name_prefix: "test-schedule-".to_string(),
        lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:test".to_string(),
        lambda_role: "arn:aws:iam::123456789012:role/test-role".to_string(),
    }
}

fn create_schedule_summary(name: &str) -> ScheduleSummary {
    ScheduleSummary::builder()
        .name(name)
        .state(aws_sdk_scheduler::types::ScheduleState::Enabled)
        .build()
}

fn create_get_schedule_output(
    name: &str,
    expression: &str,
    timezone: &str,
    arn: &str,
    description: &str,
) -> GetScheduleOutput {
    let target = Target::builder()
        .arn(arn)
        .role_arn("arn:aws:iam::123456789012:role/test-role")
        .build()
        .unwrap();

    let flexible_time_window = FlexibleTimeWindow::builder()
        .mode(FlexibleTimeWindowMode::Off)
        .build()
        .unwrap();

    GetScheduleOutput::builder()
        .name(name)
        .schedule_expression(expression)
        .schedule_expression_timezone(timezone)
        .target(target)
        .flexible_time_window(flexible_time_window)
        .description(description)
        .build()
}

#[tokio::test]
async fn test_convert_to_schedule() -> Result<(), AppError> {
    let list_rule = mock!(Client::list_schedules).then_output(|| ListSchedulesOutput::builder().build().unwrap());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule]);
    let scheduler = create_mock_scheduler(client);

    let timestamp = Utc::now().timestamp();
    let name = format!("test-schedule-{}", timestamp);
    let output = create_get_schedule_output(
        &name,
        "at(2024-01-15T10:00:00)",
        "America/New_York",
        "arn:aws:lambda:us-east-1:123456789012:function:test",
        "Test schedule",
    );

    let schedule = scheduler.convert_to_schedule(&output);
    assert_eq!(schedule.name, Some(name.clone()));
    assert_eq!(schedule.next_scheduled_timestamp_utc, Some(timestamp));
    assert_eq!(schedule.expression, Some("at(2024-01-15T10:00:00)".to_string()));
    assert_eq!(schedule.expression_timezone, Some("America/New_York".to_string()));
    assert_eq!(schedule.target, Some("arn:aws:lambda:us-east-1:123456789012:function:test".to_string()));
    assert_eq!(schedule.description, Some("Test schedule".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_convert_to_schedule_invalid_timestamp() -> Result<(), AppError> {
    let list_rule = mock!(Client::list_schedules).then_output(|| ListSchedulesOutput::builder().build().unwrap());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule]);
    let scheduler = create_mock_scheduler(client);

    let output = create_get_schedule_output(
        "test-schedule-invalid",
        "at(2024-01-15T10:00:00)",
        "America/New_York",
        "arn:aws:lambda:us-east-1:123456789012:function:test",
        "Test schedule",
    );

    let schedule = scheduler.convert_to_schedule(&output);
    assert_eq!(schedule.name, Some("test-schedule-invalid".to_string()));
    assert_eq!(schedule.next_scheduled_timestamp_utc, None);

    Ok(())
}

#[tokio::test]
async fn test_get_next_schedule_returns_valid_schedule() -> Result<(), AppError> {
    let list_rule = mock!(Client::list_schedules).then_output(|| ListSchedulesOutput::builder().build().unwrap());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule]);
    let scheduler = create_mock_scheduler(client);

    let now = Utc::now().timestamp();
    let future_time_1 = now + 3600; // 1 hour from now
    let future_time_2 = now + 7200; // 2 hours from now

    let schedules = vec![
        EventBridgeSchedule {
            name: Some("schedule-1".to_string()),
            next_scheduled_timestamp_utc: Some(future_time_1),
            schedule_id: None,
            expression: Some("at(2024-01-15T10:00:00)".to_string()),
            expression_timezone: Some("UTC".to_string()),
            target: None,
            description: None,
        },
        EventBridgeSchedule {
            name: Some("schedule-2".to_string()),
            next_scheduled_timestamp_utc: Some(future_time_2),
            schedule_id: None,
            expression: Some("at(2024-01-15T11:00:00)".to_string()),
            expression_timezone: Some("UTC".to_string()),
            target: None,
            description: None,
        },
    ];

    let result = scheduler.next_schedule(&schedules, future_time_2 + 1);
    assert!(result.is_some());
    let next = result.unwrap();
    assert_eq!(next.name, Some("schedule-1".to_string()));
    assert_eq!(next.next_scheduled_timestamp_utc, Some(future_time_1));

    Ok(())
}

#[tokio::test]
async fn test_get_next_schedule_returns_none_when_no_valid_schedule() -> Result<(), AppError> {
    let list_rule = mock!(Client::list_schedules).then_output(|| ListSchedulesOutput::builder().build().unwrap());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule]);
    let scheduler = create_mock_scheduler(client);

    let now = Utc::now().timestamp();
    let future_time = now + 3600;

    let schedules = vec![EventBridgeSchedule {
        name: Some("schedule-1".to_string()),
        next_scheduled_timestamp_utc: Some(future_time),
        schedule_id: None,
        expression: None,
        expression_timezone: None,
        target: None,
        description: None,
    }];

    // Looking for a schedule before the only available one
    let result = scheduler.next_schedule(&schedules, future_time - 1);
    assert!(result.is_none());

    Ok(())
}

#[tokio::test]
async fn test_get_next_schedule_ignores_past_schedules() -> Result<(), AppError> {
    let list_rule = mock!(Client::list_schedules).then_output(|| ListSchedulesOutput::builder().build().unwrap());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule]);
    let scheduler = create_mock_scheduler(client);

    let now = Utc::now().timestamp();
    let past_time = now - 3600; // 1 hour ago
    let future_time = now + 3600; // 1 hour from now

    let schedules = vec![
        EventBridgeSchedule {
            name: Some("past-schedule".to_string()),
            next_scheduled_timestamp_utc: Some(past_time),
            schedule_id: None,
            expression: None,
            expression_timezone: None,
            target: None,
            description: None,
        },
        EventBridgeSchedule {
            name: Some("future-schedule".to_string()),
            next_scheduled_timestamp_utc: Some(future_time),
            schedule_id: None,
            expression: None,
            expression_timezone: None,
            target: None,
            description: None,
        },
    ];

    let result = scheduler.next_schedule(&schedules, future_time + 1);
    assert!(result.is_some());
    let next = result.unwrap();
    assert_eq!(next.name, Some("future-schedule".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_delete_schedules() -> Result<(), AppError> {
    let delete_rule = mock!(Client::delete_schedule)
        .match_requests(|req| {
            assert_eq!(req.name(), Some("test-schedule-123"));
            true
        })
        .then_output(|| DeleteScheduleOutput::builder().build());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&delete_rule]);
    let scheduler = create_mock_scheduler(client);

    scheduler.delete_schedules("test-schedule-123").await?;

    Ok(())
}

#[tokio::test]
async fn test_list_schedules_empty() -> Result<(), AppError> {
    let list_rule = mock!(Client::list_schedules)
        .match_requests(|req| {
            assert_eq!(req.name_prefix(), Some("test-schedule-"));
            true
        })
        .then_output(|| {
            ListSchedulesOutput::builder()
                .set_schedules(Some(vec![]))
                .build()
                .unwrap()
        });

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule]);
    let scheduler = create_mock_scheduler(client);

    let schedules = scheduler.list_schedules().await?;
    assert_eq!(schedules.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_list_schedules_with_items() -> Result<(), AppError> {
    let timestamp = Utc::now().timestamp();
    let schedule_name = format!("test-schedule-{}", timestamp);
    let schedule_name_clone1 = schedule_name.clone();
    let schedule_name_clone2 = schedule_name.clone();

    let list_rule = mock!(Client::list_schedules).then_output(move || {
        ListSchedulesOutput::builder()
            .schedules(create_schedule_summary(&schedule_name_clone1))
            .build()
            .unwrap()
    });

    let get_rule = mock!(Client::get_schedule).then_output(move || {
        create_get_schedule_output(
            &schedule_name_clone2,
            &format!("at({})", Utc.timestamp_opt(timestamp, 0).unwrap().format("%FT%T")),
            "UTC",
            "arn:aws:lambda:us-east-1:123456789012:function:test",
            "Test schedule",
        )
    });

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule, &get_rule]);
    let scheduler = create_mock_scheduler(client);

    let schedules = scheduler.list_schedules().await?;
    assert_eq!(schedules.len(), 1);
    assert_eq!(schedules[0].name.as_deref(), Some(schedule_name.as_str()));

    Ok(())
}

#[tokio::test]
async fn test_cleanup_schedules_removes_future_schedules() -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let next_schedule_time = now + 3600; // 1 hour from now
    let future_schedule_time = now + 7200; // 2 hours from now

    let schedules = vec![EventBridgeSchedule {
        name: Some("test-schedule-future".to_string()),
        next_scheduled_timestamp_utc: Some(future_schedule_time),
        schedule_id: None,
        expression: None,
        expression_timezone: None,
        target: None,
        description: None,
    }];

    let delete_rule = mock!(Client::delete_schedule)
        .match_requests(|req| {
            assert_eq!(req.name(), Some("test-schedule-future"));
            true
        })
        .then_output(|| DeleteScheduleOutput::builder().build());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&delete_rule]);
    let scheduler = create_mock_scheduler(client);

    scheduler.cleanup_schedules(schedules, next_schedule_time).await?;

    Ok(())
}

#[tokio::test]
async fn test_cleanup_schedules_removes_outdated_schedules() -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let outdated_time = now - 600; // 10 minutes ago (past the 5 minute threshold)
    let next_schedule_time = now + 3600;

    let schedules = vec![EventBridgeSchedule {
        name: Some("test-schedule-outdated".to_string()),
        next_scheduled_timestamp_utc: Some(outdated_time),
        schedule_id: None,
        expression: None,
        expression_timezone: None,
        target: None,
        description: None,
    }];

    let delete_rule = mock!(Client::delete_schedule)
        .match_requests(|req| {
            assert_eq!(req.name(), Some("test-schedule-outdated"));
            true
        })
        .then_output(|| DeleteScheduleOutput::builder().build());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&delete_rule]);
    let scheduler = create_mock_scheduler(client);

    scheduler.cleanup_schedules(schedules, next_schedule_time).await?;

    Ok(())
}

#[tokio::test]
async fn test_cleanup_schedules_keeps_next_schedule() -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let next_schedule_time = now + 3600;

    let schedules = vec![EventBridgeSchedule {
        name: Some("test-schedule-keep".to_string()),
        next_scheduled_timestamp_utc: Some(next_schedule_time),
        schedule_id: None,
        expression: None,
        expression_timezone: None,
        target: None,
        description: None,
    }];

    // No delete should be called
    let list_rule = mock!(Client::list_schedules).then_output(|| ListSchedulesOutput::builder().build().unwrap());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule]);
    let scheduler = create_mock_scheduler(client);

    scheduler.cleanup_schedules(schedules, next_schedule_time).await?;

    Ok(())
}

#[tokio::test]
async fn test_cleanup_schedules_with_multiple_schedules() -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let next_schedule_time = now + 3600;
    let outdated_time = now - 600;
    let future_time = now + 7200;

    let schedules = vec![
        EventBridgeSchedule {
            name: Some("keep-schedule".to_string()),
            next_scheduled_timestamp_utc: Some(next_schedule_time),
            schedule_id: None,
            expression: None,
            expression_timezone: None,
            target: None,
            description: None,
        },
        EventBridgeSchedule {
            name: Some("delete-outdated".to_string()),
            next_scheduled_timestamp_utc: Some(outdated_time),
            schedule_id: None,
            expression: None,
            expression_timezone: None,
            target: None,
            description: None,
        },
        EventBridgeSchedule {
            name: Some("delete-future".to_string()),
            next_scheduled_timestamp_utc: Some(future_time),
            schedule_id: None,
            expression: None,
            expression_timezone: None,
            target: None,
            description: None,
        },
    ];

    let delete_rule_1 = mock!(Client::delete_schedule)
        .match_requests(|req| {
            assert_eq!(req.name(), Some("delete-outdated"));
            true
        })
        .then_output(|| DeleteScheduleOutput::builder().build());

    let delete_rule_2 = mock!(Client::delete_schedule)
        .match_requests(|req| {
            assert_eq!(req.name(), Some("delete-future"));
            true
        })
        .then_output(|| DeleteScheduleOutput::builder().build());

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&delete_rule_1, &delete_rule_2]);
    let scheduler = create_mock_scheduler(client);

    scheduler.cleanup_schedules(schedules, next_schedule_time).await?;

    Ok(())
}

#[tokio::test]
async fn test_update_next_schedule_creates_new_when_earlier() -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let existing_schedule_time = now + 7200; // 2 hours from now
    let new_schedule_time = now + 3600; // 1 hour from now (earlier)

    let new_datetime = Utc
        .timestamp_opt(new_schedule_time, 0)
        .unwrap()
        .with_timezone(&New_York);

    let cron_schedule = CronSchedule {
        cron: "0 9 * * *".to_string(),
        timezone: New_York,
        next_oneoff_cron: format!(
            "{} {} {} {} * {}",
            new_datetime.minute(),
            new_datetime.hour(),
            new_datetime.day(),
            new_datetime.month(),
            new_datetime.year()
        ),
        next_timestamp_utc: new_schedule_time,
        next_datetime: new_datetime,
    };

    // Mock list_schedules to return existing schedule
    let list_rule = mock!(Client::list_schedules).then_output(move || {
        ListSchedulesOutput::builder()
            .schedules(create_schedule_summary(&format!("test-schedule-{}", existing_schedule_time)))
            .build()
            .unwrap()
    });

    let get_rule = mock!(Client::get_schedule).then_output(move || {
        create_get_schedule_output(
            &format!("test-schedule-{}", existing_schedule_time),
            &format!("at({})", Utc.timestamp_opt(existing_schedule_time, 0).unwrap().format("%FT%T")),
            "UTC",
            "arn:aws:lambda:us-east-1:123456789012:function:test",
            "Existing schedule",
        )
    });

    // Mock create_schedule for new schedule
    let create_rule = mock!(Client::create_schedule)
        .match_requests(move |req| {
            assert_eq!(req.name(), Some(format!("test-schedule-{}", new_schedule_time).as_str()));
            assert!(req.schedule_expression().unwrap().starts_with("at("));
            true
        })
        .then_output(|| {
            aws_sdk_scheduler::operation::create_schedule::CreateScheduleOutput::builder()
                .schedule_arn("arn:aws:scheduler:us-east-1:123456789012:schedule/test-schedule")
                .build()
                .unwrap()
        });

    // Mock delete_schedule for cleanup
    let delete_rule = mock!(Client::delete_schedule)
        .match_requests(move |req| {
            assert_eq!(req.name(), Some(format!("test-schedule-{}", existing_schedule_time).as_str()));
            true
        })
        .then_output(|| DeleteScheduleOutput::builder().build());

    let client =
        mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule, &get_rule, &create_rule, &delete_rule]);
    let scheduler = create_mock_scheduler(client);

    scheduler.update_next_schedule(&cron_schedule).await?;

    Ok(())
}

#[tokio::test]
async fn test_update_next_schedule_keeps_existing_when_later() -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let existing_schedule_time = now + 3600; // 1 hour from now
    let new_schedule_time = now + 7200; // 2 hours from now (later)

    let new_datetime = Utc
        .timestamp_opt(new_schedule_time, 0)
        .unwrap()
        .with_timezone(&New_York);

    let cron_schedule = CronSchedule {
        cron: "0 9 * * *".to_string(),
        timezone: New_York,
        next_oneoff_cron: format!(
            "{} {} {} {} * {}",
            new_datetime.minute(),
            new_datetime.hour(),
            new_datetime.day(),
            new_datetime.month(),
            new_datetime.year()
        ),
        next_timestamp_utc: new_schedule_time,
        next_datetime: new_datetime,
    };

    // Mock list_schedules to return existing schedule
    let list_rule = mock!(Client::list_schedules).then_output(move || {
        ListSchedulesOutput::builder()
            .schedules(create_schedule_summary(&format!("test-schedule-{}", existing_schedule_time)))
            .build()
            .unwrap()
    });

    let get_rule = mock!(Client::get_schedule).then_output(move || {
        create_get_schedule_output(
            &format!("test-schedule-{}", existing_schedule_time),
            &format!("at({})", Utc.timestamp_opt(existing_schedule_time, 0).unwrap().format("%FT%T")),
            "UTC",
            "arn:aws:lambda:us-east-1:123456789012:function:test",
            "Existing schedule",
        )
    });

    // No create_schedule should be called
    // No delete_schedule should be called (schedule is within valid range)

    let client = mock_client!(aws_sdk_scheduler, RuleMode::Sequential, &[&list_rule, &get_rule]);
    let scheduler = create_mock_scheduler(client);

    scheduler.update_next_schedule(&cron_schedule).await?;

    Ok(())
}
