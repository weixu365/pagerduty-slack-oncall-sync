use crate::db::ScheduledTask;
use crate::slack_handler::morphism_patches::blocks_kit::{SlackBlock, SlackModalView, SlackView};
use chrono::{DateTime, SecondsFormat};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use slack_morphism::prelude::*;

pub const DEFAULT_PAGE_SIZE: usize = 5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleFilter {
    Auto,
    All,
    User,
    Channel,
}

impl ScheduleFilter {
    /// All filters, in the order they appear in the dropdown.
    const ALL: [ScheduleFilter; 4] = [
        ScheduleFilter::Auto,
        ScheduleFilter::All,
        ScheduleFilter::User,
        ScheduleFilter::Channel,
    ];

    fn label(&self) -> &'static str {
        match self {
            ScheduleFilter::Auto => "Auto (My + Channel)",
            ScheduleFilter::All => "All Schedules",
            ScheduleFilter::User => "My Schedules",
            ScheduleFilter::Channel => "Channel Schedules",
        }
    }
}

pub fn build_schedule_list_view(
    tasks: &[ScheduledTask],
    page: usize,
    page_size: usize,
    user_id: &str,
    channel_id: Option<&String>,
    filter: &ScheduleFilter,
    next_trigger_timestamp: Option<i64>,
    is_admin: bool,
) -> SlackView {
    // Filter tasks based on the selected filter
    let filtered_tasks: Vec<&ScheduledTask> = match filter {
        ScheduleFilter::All => tasks.iter().collect(),
        ScheduleFilter::User => tasks.iter().filter(|t| t.created_by_user_id == user_id).collect(),
        ScheduleFilter::Channel => tasks.iter().filter(|t| channel_id == Some(&t.channel_id)).collect(),
        ScheduleFilter::Auto => tasks
            .iter()
            .filter(|t| t.created_by_user_id == user_id || channel_id == Some(&t.channel_id))
            .collect(),
    };

    let total_items = filtered_tasks.len();
    let total_pages = total_items.div_ceil(page_size);
    let current_page = page.min(total_pages.saturating_sub(1));

    let start_idx = current_page * page_size;
    let end_idx = (start_idx + page_size).min(total_items);
    let page_tasks = &filtered_tasks[start_idx..end_idx];

    let mut blocks: Vec<SlackBlock> = Vec::new();

    // Header
    blocks.push(SlackBlock::Header(
        SlackHeaderBlock::new("📋 Scheduled Tasks".into())
            .with_block_id(format!("header_{}", chrono::Utc::now()).into()),
    ));

    // Filter dropdown
    let filter_select = SlackBlockStaticSelectElement::new()
        .with_action_id("filter_select".into())
        .with_placeholder("Filter by".into())
        .with_options(
            ScheduleFilter::ALL
                .iter()
                .map(|f| SlackBlockChoiceItem::new(f.label().into(), list_view_state(0, page_size, f)))
                .collect(),
        )
        .with_initial_option(SlackBlockChoiceItem::new(filter.label().into(), list_view_state(0, page_size, filter)));

    let page_size_select = SlackBlockStaticSelectElement::new()
        .with_action_id("page_size_select".into())
        .with_placeholder("Items per page".into())
        .with_options(vec![
            SlackBlockChoiceItem::new("5".into(), list_view_state(0, 5, filter)),
            SlackBlockChoiceItem::new("10".into(), list_view_state(0, 10, filter)),
        ])
        .with_initial_option(SlackBlockChoiceItem::new(
            page_size.to_string().into(),
            list_view_state(0, page_size, filter),
        ));

    // Add "New" button
    let new_button = SlackBlockButtonElement::new("Create New".into())
        .with_action_id("new_schedule".into())
        .with_style(SlackBlockButtonStyle::Primary);

    let mut action_elements = vec![
        SlackActionBlockElement::StaticSelect(filter_select),
        SlackActionBlockElement::StaticSelect(page_size_select),
        SlackActionBlockElement::Button(new_button),
    ];

    if is_admin {
        let sync_button = SlackBlockButtonElement::new("⚡ Sync Now".into())
            .with_action_id("sync_now".into())
            .with_value(list_view_state(current_page, page_size, filter))
            .with_style(SlackBlockButtonStyle::Primary);
        action_elements.push(SlackActionBlockElement::Button(sync_button));
    }

    blocks.push(SlackBlock::Actions(SlackActionsBlock::new(action_elements)));

    blocks.push(SlackBlock::Divider(SlackDividerBlock::new()));

    // No tasks found
    if page_tasks.is_empty() {
        blocks.push(SlackBlock::Section(SlackSectionBlock::new().with_text(md!("_No scheduled tasks found._"))));
    }

    // Schedule items
    for task in page_tasks.iter() {
        let blocks_for_task = build_schedule_item_blocks(task, current_page, page_size, user_id, filter);
        blocks.extend(blocks_for_task);
    }

    // Always show controls for refresh button and pagination controls
    blocks.extend(build_pagination_blocks(
        start_idx,
        end_idx,
        total_items,
        current_page,
        total_pages,
        page_size,
        filter,
        next_trigger_timestamp,
    ));

    SlackView::Modal(SlackModalView::new("Scheduled Tasks".into(), blocks))
}

fn build_schedule_item_blocks(
    task: &ScheduledTask,
    page: usize,
    page_size: usize,
    user_id: &str,
    filter: &ScheduleFilter,
) -> Vec<SlackBlock> {
    let mut blocks = Vec::new();

    let last_updated_formatted = DateTime::parse_from_rfc3339(&task.last_updated_at)
        .ok()
        .and_then(|dt| {
            let tz: Result<Tz, _> = task.timezone.parse();
            tz.ok()
                .map(|tz| dt.with_timezone(&tz).to_rfc3339_opts(SecondsFormat::Secs, true))
        })
        .unwrap_or_else(|| task.last_updated_at.clone());

    let text_content = format!(
        "*<#{}>* | *@{}* by <@{}>\nScheduled at: `{}` `{}`\nUpdated At: `{}`\nNext Run: `{}`",
        task.channel_id,
        task.user_group_handle,
        task.created_by_user_id,
        task.cron,
        task.timezone,
        last_updated_formatted,
        task.next_update_time,
    );

    let section = if user_id == task.created_by_user_id {
        let delete_value = serde_json::json!({
            "team_id": task.team_id,
            "enterprise_id": task.enterprise_id,
            "task_id": task.task_id,
            "page": page,
            "page_size": page_size,
            "filter": filter,
        })
        .to_string();

        let delete_button = SlackBlockButtonElement::new("Delete".into())
            .with_action_id("delete_schedule".into())
            .with_value(delete_value)
            .with_style(SlackBlockButtonStyle::Danger)
            .with_confirm(
                SlackBlockConfirmItem::new(
                    "Delete Schedule?".into(),
                    md!(format!(
                        "Are you sure you want to delete the schedule for #{} / @{}?\nLast updated at {}",
                        task.channel_name, task.user_group_handle, last_updated_formatted
                    )),
                    "Delete".into(),
                    "Cancel".into(),
                )
                .with_style("danger".into()),
            );

        SlackSectionBlock::new()
            .with_text(md!(text_content))
            .with_accessory(SlackSectionBlockElement::Button(delete_button))
    } else {
        SlackSectionBlock::new().with_text(md!(text_content))
    };

    // Main info section
    blocks.push(SlackBlock::Section(section));

    blocks.push(SlackBlock::Divider(SlackDividerBlock::new()));

    blocks
}

/// Serialised list view state (page, page size, filter) sent as the Slack value for buttons and dropdown options.
fn list_view_state(page: usize, page_size: usize, filter: &ScheduleFilter) -> String {
    serde_json::json!({
        "page": page,
        "page_size": page_size,
        "filter": filter,
    })
    .to_string()
}

fn build_pagination_blocks(
    start_idx: usize,
    end_idx: usize,
    total_items: usize,
    current_page: usize,
    total_pages: usize,
    page_size: usize,
    filter: &ScheduleFilter,
    next_trigger_timestamp: Option<i64>,
) -> Vec<SlackBlock> {
    let mut blocks = Vec::new();

    let mut button_elements = Vec::new();

    // Refresh button
    button_elements.push(SlackActionBlockElement::Button(
        SlackBlockButtonElement::new("🔄 Refresh".into())
            .with_action_id("refresh".into())
            .with_value(list_view_state(current_page, page_size, filter)),
    ));

    // Previous button
    if current_page > 0 {
        button_elements.push(SlackActionBlockElement::Button(
            SlackBlockButtonElement::new("← Previous".into())
                .with_action_id("page_previous".into())
                .with_value(list_view_state(current_page - 1, page_size, filter)),
        ));
    }

    // Next button
    if current_page + 1 < total_pages {
        button_elements.push(SlackActionBlockElement::Button(
            SlackBlockButtonElement::new("Next →".into())
                .with_action_id("page_next".into())
                .with_value(list_view_state(current_page + 1, page_size, filter)),
        ));
    }

    // Actions block with buttons
    blocks.push(SlackBlock::Actions(SlackActionsBlock::new(button_elements)));

    blocks.push(SlackBlock::Context(SlackContextBlock::new(vec![SlackContextBlockElement::MarkDown(
        SlackBlockMarkDownText::new(format!(
            "{}: Showing {} - {} of {} schedules on page {} of {}. Next trigger: {}",
            current_time_markdown(),
            start_idx + 1,
            end_idx,
            total_items,
            current_page + 1,
            total_pages,
            timestamp_markdown(next_trigger_timestamp),
        )),
    )])));

    blocks
}

/// Generate a Slack markdown timestamp for the current time
/// https://docs.slack.dev/messaging/formatting-message-text/#date-formatting
fn current_time_markdown() -> String {
    let now = chrono::Utc::now();

    format!("<!date^{}^{{date_pretty}} {{time_secs}}|{}>", now.timestamp(), now.to_rfc3339())
}

/// Generate a Slack markdown timestamp for the current time
/// https://docs.slack.dev/messaging/formatting-message-text/#date-formatting
fn timestamp_markdown(timestamp: Option<i64>) -> String {
    if let Some(timestamp_seconds) = timestamp
        && let Some(time) = chrono::DateTime::from_timestamp(timestamp_seconds, 0) {
            return format!("<!date^{}^{{date_pretty}} {{time_secs}}|{}>", time.timestamp(), time.to_rfc3339());
        }

    "".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack_handler::morphism_patches::blocks_kit::SlackView;
    use chrono::Utc;

    fn create_test_task(channel: &str, group: &str) -> ScheduledTask {
        ScheduledTask {
            team: "T123:E456".to_string(),
            task_id: "task_1".to_string(),
            next_update_timestamp_utc: Utc::now().timestamp(),
            next_update_time: "2024-01-15T09:00:00Z".to_string(),
            team_id: "T123".to_string(),
            team_domain: "test.slack.com".to_string(),
            channel_id: "C123".to_string(),
            channel_name: channel.to_string(),
            enterprise_id: "E456".to_string(),
            enterprise_name: "Test Enterprise".to_string(),
            is_enterprise_install: false,
            user_group_id: "S123".to_string(),
            user_group_handle: group.to_string(),
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

    #[test]
    fn test_build_schedule_list_empty() {
        let tasks: Vec<ScheduledTask> = vec![];
        let view = build_schedule_list_view(
            &tasks,
            0,
            10,
            "U123",
            Some(&"C123".to_string()),
            &ScheduleFilter::Auto,
            None,
            false,
        );

        // Verify it's a Modal view
        match &view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty(), "Blocks should have at least one item");
            }
            _ => panic!("Expected SlackView::Modal"),
        }
    }

    #[test]
    fn test_build_schedule_list_single_page() {
        let tasks = vec![
            create_test_task("general", "oncall"),
            create_test_task("engineering", "eng-oncall"),
        ];
        let view = build_schedule_list_view(
            &tasks,
            0,
            10,
            "U123",
            Some(&"C123".to_string()),
            &ScheduleFilter::Auto,
            None,
            false,
        );

        // Verify it's a Modal view
        match &view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty(), "Blocks should have at least one item");
            }
            _ => panic!("Expected SlackView::Modal"),
        }
    }

    #[test]
    fn test_build_schedule_list_pagination() {
        // Create 25 tasks to test pagination (PAGE_SIZE = 10)
        let mut tasks = Vec::new();
        for i in 0..25 {
            tasks.push(create_test_task(&format!("channel{}", i), &format!("group{}", i)));
        }

        // Page 0
        let view = build_schedule_list_view(
            &tasks,
            0,
            10,
            "U123",
            Some(&"C123".to_string()),
            &ScheduleFilter::Auto,
            None,
            false,
        );

        // Verify it's a Modal view
        match &view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty(), "Blocks should have at least one item");
            }
            _ => panic!("Expected SlackView::Modal"),
        }

        // Page 1
        let view = build_schedule_list_view(
            &tasks,
            1,
            10,
            "U123",
            Some(&"C123".to_string()),
            &ScheduleFilter::Auto,
            None,
            false,
        );

        // Verify it's a Modal view
        match &view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty(), "Blocks should have at least one item");
            }
            _ => panic!("Expected SlackView::Modal"),
        }
    }
}
