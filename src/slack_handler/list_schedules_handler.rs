use crate::{db::ScheduledTaskRepository, errors::AppError};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ScheduledTask;
    use async_trait::async_trait;
    use chrono::Utc;

    struct MockScheduledTaskRepository {
        tasks: Vec<ScheduledTask>,
    }

    #[async_trait]
    impl ScheduledTaskRepository for MockScheduledTaskRepository {
        async fn save_scheduled_task(&self, _task: &ScheduledTask) -> Result<(), AppError> {
            Ok(())
        }

        async fn update_next_schedule(&self, _task: &ScheduledTask) -> Result<(), AppError> {
            Ok(())
        }

        async fn list_scheduled_tasks_in_workspace(
            &self,
            _workspace_id: &String,
            _workspace_name: &String,
        ) -> Result<(), AppError> {
            Ok(())
        }

        async fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, AppError> {
            Ok(self.tasks.clone())
        }

        async fn delete_scheduled_task(
            &self,
            _team_id: &str,
            _workspace_id: &str,
            _task_id: &str,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    fn create_test_task(
        channel_name: &str,
        user_group_handle: &str,
        cron: &str,
        next_update_time: &str,
    ) -> ScheduledTask {
        ScheduledTask {
            team: "T123:E456".to_string(),
            task_id: "task_1".to_string(),
            next_update_timestamp_utc: Utc::now().timestamp(),
            next_update_time: next_update_time.to_string(),

            team_id: "T123".to_string(),
            team_domain: "test.slack.com".to_string(),
            channel_id: "C123".to_string(),
            channel_name: channel_name.to_string(),
            enterprise_id: "E456".to_string(),
            enterprise_name: "Test Enterprise".to_string(),
            is_enterprise_install: false,

            user_group_id: "S123".to_string(),
            user_group_handle: user_group_handle.to_string(),
            pager_duty_schedule_id: "PD123".to_string(),
            pager_duty_token: None,
            cron: cron.to_string(),
            timezone: "UTC".to_string(),

            created_by_user_id: "U123".to_string(),
            created_by_user_name: "testuser".to_string(),
            created_at: Utc::now().to_rfc3339(),
            last_updated_at: Utc::now().to_rfc3339(),
        }
    }

    #[tokio::test]
    async fn test_handle_list_schedules_command_empty() -> Result<(), AppError> {
        let mock_db = MockScheduledTaskRepository { tasks: vec![] };

        let schedules = handle_list_schedules_command(&mock_db).await?;
        assert_eq!(schedules.len(), 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_list_schedules_command_single_task() -> Result<(), AppError> {
        let task = create_test_task("general", "oncall", "0 9 * * *", "2024-01-15T09:00:00Z");
        let mock_db = MockScheduledTaskRepository { tasks: vec![task] };

        let schedules = handle_list_schedules_command(&mock_db).await?;
        assert_eq!(schedules.len(), 1);
        assert!(schedules[0].contains("## general"));
        assert!(schedules[0].contains("Update oncall on 0 9 * * *"));
        assert!(schedules[0].contains("Next schedule: 2024-01-15T09:00:00Z"));

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_list_schedules_command_multiple_tasks() -> Result<(), AppError> {
        let task1 = create_test_task("general", "oncall", "0 9 * * *", "2024-01-15T09:00:00Z");
        let task2 = create_test_task("engineering", "on-call-eng", "0 10 * * *", "2024-01-15T10:00:00Z");
        let task3 = create_test_task("ops", "ops-team", "0 8 * * MON-FRI", "2024-01-16T08:00:00Z");

        let mock_db = MockScheduledTaskRepository {
            tasks: vec![task1, task2, task3],
        };

        let schedules = handle_list_schedules_command(&mock_db).await?;
        assert_eq!(schedules.len(), 3);

        assert!(schedules[0].contains("## general"));
        assert!(schedules[0].contains("oncall"));
        assert!(schedules[1].contains("## engineering"));
        assert!(schedules[1].contains("on-call-eng"));
        assert!(schedules[2].contains("## ops"));
        assert!(schedules[2].contains("ops-team"));

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_list_schedules_command_format() -> Result<(), AppError> {
        let task = create_test_task("my-channel", "my-group", "0 */2 * * *", "2024-12-25T14:00:00Z");
        let mock_db = MockScheduledTaskRepository { tasks: vec![task] };

        let schedules = handle_list_schedules_command(&mock_db).await?;
        assert_eq!(schedules.len(), 1);

        let schedule_text = &schedules[0];
        assert!(schedule_text.starts_with("## "));
        assert!(schedule_text.contains("my-channel"));
        assert!(schedule_text.contains("\nUpdate my-group on 0 */2 * * *\n"));
        assert!(schedule_text.contains("Next schedule: 2024-12-25T14:00:00Z"));

        Ok(())
    }
}
