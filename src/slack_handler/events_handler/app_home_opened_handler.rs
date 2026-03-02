use crate::aws::event_bridge_scheduler::EventBridgeScheduler;
use crate::db::SlackInstallationRepository;
use crate::service::slack::publish_slack_view;
use crate::slack_handler::morphism_patches::blocks_kit::{SlackHomeView, SlackView};
use crate::slack_handler::views::schedule_list::{ScheduleFilter, build_schedule_list_view};
use crate::{db::ScheduledTaskRepository, errors::AppError};

pub struct AppHomeOpenedEvent {
    pub user_id: String,
    pub team_id: String,
    pub enterprise_id: String,
}

pub async fn app_home_opened(
    event: &AppHomeOpenedEvent,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    slack_installations_db: &dyn SlackInstallationRepository,
    scheduler: &EventBridgeScheduler,
    page_size: usize,
    is_admin: bool,
) -> Result<(), AppError> {
    let installation = slack_installations_db
        .get_slack_installation(&event.team_id, &event.enterprise_id)
        .await?;

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;

    let next_trigger_timestamp = scheduler
        .get_current_schedule()
        .await?
        .and_then(|s| s.next_scheduled_timestamp_utc);

    let view = build_schedule_list_view(
        &tasks,
        0,
        page_size,
        &event.user_id,
        None,
        &ScheduleFilter::Auto,
        next_trigger_timestamp,
        is_admin,
    );

    if let SlackView::Modal(modal_view) = &view {
        let home_view = SlackView::Home(SlackHomeView::new(modal_view.blocks.clone()));
        publish_slack_view(&home_view, &event.user_id, &installation.access_token).await?;
        Ok(())
    } else {
        Err(AppError::Error("Expected a modal view for the home tab".to_string()))
    }
}
