use crate::{db::ScheduledTaskRepository, errors::AppError};
use serde::{Deserialize, Serialize};
use slack_morphism::prelude::*;
use super::block_kit::{build_schedule_list_blocks, decode_schedule_id, DEFAULT_PAGE_SIZE};

#[derive(Debug, Deserialize)]
pub struct InteractivePayload {
    #[serde(rename = "type")]
    pub payload_type: String,
    pub user: InteractiveUser,
    pub team: InteractiveTeam,
    pub actions: Option<Vec<BlockAction>>,
    pub response_url: String,
}

#[derive(Debug, Deserialize)]
pub struct InteractiveUser {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct InteractiveTeam {
    pub id: String,
    pub domain: String,
}

#[derive(Debug, Deserialize)]
pub struct BlockAction {
    pub action_id: String,
    pub block_id: Option<String>,
    pub value: Option<String>,
    pub selected_option: Option<SelectedOption>,
}

#[derive(Debug, Deserialize)]
pub struct SelectedOption {
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct InteractiveResponse {
    pub slack_view: SlackView,
    pub replace_original: bool,
}

pub async fn handle_interactive_action(
    payload: InteractivePayload,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
) -> Result<InteractiveResponse, AppError> {
    tracing::info!(payload = ?payload, "Handling interactive action");

    if payload.payload_type != "block_actions" {
        return Err(AppError::InvalidData(format!(
            "Unsupported payload type: {}",
            payload.payload_type
        )));
    }

    let actions = payload.actions.ok_or_else(|| {
        AppError::InvalidData("No actions found in payload".to_string())
    })?;

    if actions.is_empty() {
        return Err(AppError::InvalidData("Empty actions list".to_string()));
    }

    let action = &actions[0];
    let action_id = &action.action_id;

    // Handle different action types
    if action_id.starts_with("delete_schedule_") {
        handle_delete_schedule(action_id, scheduled_tasks_db).await
    } else if action_id.starts_with("refresh_page_") {
        handle_refresh(action_id, scheduled_tasks_db).await
    } else if action_id == "page_size_select" {
        handle_page_size_change(action, scheduled_tasks_db).await
    } else if action_id.starts_with("page_") {
        handle_pagination(action_id, scheduled_tasks_db).await
    } else {
        Err(AppError::InvalidData(format!(
            "Unknown action_id: {}",
            action_id
        )))
    }
}

async fn handle_delete_schedule(
    action_id: &str,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
) -> Result<InteractiveResponse, AppError> {
    tracing::info!(action_id = %action_id, "Deleting schedule");
    let encoded_id = action_id
        .strip_prefix("delete_schedule_")
        .ok_or_else(|| AppError::InvalidData("Invalid delete action_id".to_string()))?;

    // Decode schedule identifiers
    let (team_id, enterprise_id, task_id) = decode_schedule_id(encoded_id)
        .ok_or_else(|| AppError::InvalidData("Failed to decode schedule ID".to_string()))?;

    // Delete the schedule
    scheduled_tasks_db
        .delete_scheduled_task(&team_id, &enterprise_id, &task_id)
        .await?;

    // Return updated list (page 0)
    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let response = build_schedule_list_blocks(&tasks, 0, DEFAULT_PAGE_SIZE);

    Ok(InteractiveResponse {
        slack_view: response.slack_view,
        replace_original: true,
    })
}

async fn handle_pagination(
    action_id: &str,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
) -> Result<InteractiveResponse, AppError> {
    tracing::info!(action_id = %action_id, "Paginating to page");
    // Extract page number
    let page_str = action_id
        .strip_prefix("page_")
        .ok_or_else(|| AppError::InvalidData("Invalid pagination action_id".to_string()))?;

    let page: usize = page_str
        .parse()
        .map_err(|_| AppError::InvalidData("Invalid page number".to_string()))?;

    // Get tasks and build response for requested page
    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let response = build_schedule_list_blocks(&tasks, page, DEFAULT_PAGE_SIZE);

    Ok(InteractiveResponse {
        slack_view: response.slack_view,
        replace_original: true,
    })
}

async fn handle_refresh(
    action_id: &str,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
) -> Result<InteractiveResponse, AppError> {
    tracing::info!(action_id = %action_id, "Refreshing page");
    
    let page_str = action_id
        .strip_prefix("refresh_page_")
        .ok_or_else(|| AppError::InvalidData("Invalid refresh action_id".to_string()))?;

    let page: usize = page_str
        .parse()
        .map_err(|_| AppError::InvalidData("Invalid page number".to_string()))?;

    // Get tasks and build response for current page
    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let response = build_schedule_list_blocks(&tasks, page, DEFAULT_PAGE_SIZE);

    Ok(InteractiveResponse {
        slack_view: response.slack_view,
        replace_original: true,
    })
}

async fn handle_page_size_change(
    action: &BlockAction,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
) -> Result<InteractiveResponse, AppError> {
    tracing::info!(action = ?action, "Changing page size");
    // Extract page size from selected option
    let page_size: usize = action
        .selected_option
        .as_ref()
        .and_then(|opt| opt.value.parse().ok())
        .unwrap_or(DEFAULT_PAGE_SIZE);

    // Get tasks and build response for page 0 with new page size
    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    let response = build_schedule_list_blocks(&tasks, 0, page_size);

    Ok(InteractiveResponse {
        slack_view: response.slack_view,
        replace_original: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ScheduledTask, ScheduledTaskRepository};
    use async_trait::async_trait;
    use chrono::Utc;

    struct MockScheduledTaskRepository {
        tasks: Vec<ScheduledTask>,
        deleted: std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
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
            team_id: &str,
            workspace_id: &str,
            task_id: &str,
        ) -> Result<(), AppError> {
            let mut deleted = self.deleted.lock().unwrap();
            deleted.push((
                team_id.to_string(),
                workspace_id.to_string(),
                task_id.to_string(),
            ));
            Ok(())
        }
    }

    fn create_test_task() -> ScheduledTask {
        ScheduledTask {
            team: "T123:E456".to_string(),
            task_id: "task_1".to_string(),
            next_update_timestamp_utc: Utc::now().timestamp(),
            next_update_time: "2024-01-15T09:00:00Z".to_string(),
            team_id: "T123".to_string(),
            team_domain: "test.slack.com".to_string(),
            channel_id: "C123".to_string(),
            channel_name: "general".to_string(),
            enterprise_id: "E456".to_string(),
            enterprise_name: "Test Enterprise".to_string(),
            is_enterprise_install: false,
            user_group_id: "S123".to_string(),
            user_group_handle: "oncall".to_string(),
            pager_duty_schedule_id: "PD123".to_string(),
            pager_duty_token: None,
            cron: "0 9 * * *".to_string(),
            timezone: "UTC".to_string(),
            created_by_user_id: "U123".to_string(),
            created_by_user_name: "testuser".to_string(),
            created_at: Utc::now().to_rfc3339(),
            last_updated_at: Utc::now().to_rfc3339(),
        }
    }

    #[tokio::test]
    async fn test_handle_pagination() {
        let tasks = vec![create_test_task()];
        let deleted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mock_db = MockScheduledTaskRepository {
            tasks,
            deleted: deleted.clone(),
        };

        let response = handle_pagination("page_0", &mock_db).await.unwrap();
        assert!(response.replace_original);

        // Verify it's a Modal view with blocks
        match &response.slack_view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty());
            }
            _ => panic!("Expected SlackView::Modal"),
        }
    }

    #[tokio::test]
    async fn test_handle_refresh() {
        let tasks = vec![create_test_task()];
        let deleted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mock_db = MockScheduledTaskRepository {
            tasks,
            deleted: deleted.clone(),
        };

        let response = handle_refresh("refresh_page_0", &mock_db).await.unwrap();
        assert!(response.replace_original);

        // Verify it's a Modal view with blocks
        match &response.slack_view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty());
            }
            _ => panic!("Expected SlackView::Modal"),
        }
    }
}
