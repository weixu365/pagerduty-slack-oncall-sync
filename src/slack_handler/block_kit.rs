use crate::db::ScheduledTask;
use slack_morphism::prelude::*;
use chrono::{DateTime, SecondsFormat};
use chrono_tz::Tz;

pub const DEFAULT_PAGE_SIZE: usize = 5;

#[derive(Debug, Clone)]
pub struct ScheduleListResponse {
    pub slack_view: SlackView,
    pub page: usize,
    pub total_pages: usize,
}

/// Build Block Kit blocks for a list of schedules with pagination
pub fn build_schedule_list_blocks(
    tasks: &[ScheduledTask],
    page: usize,
    page_size: usize,
) -> ScheduleListResponse {
    let total_items = tasks.len();
    let total_pages = (total_items + page_size - 1) / page_size; // Ceiling division
    let current_page = page.min(total_pages.saturating_sub(1));

    let start_idx = current_page * page_size;
    let end_idx = (start_idx + page_size).min(total_items);
    let page_tasks = &tasks[start_idx..end_idx];

    let mut blocks: Vec<SlackBlock> = Vec::new();

    // Header
    blocks.push(
        SlackBlock::Header(
            SlackHeaderBlock::new(
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("📋 Scheduled Tasks".into()).with_emoji(true))
            ).with_block_id(format!("header_{}", chrono::Utc::now()).into())
        )
    );

    if page_tasks.is_empty() {
        blocks.push(SlackBlock::Section(SlackSectionBlock::new().with_text(md!("_No scheduled tasks found._"))));

        let slack_view = SlackView::Modal(
            SlackModalView::new(
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Scheduled Tasks".into())),
                blocks
            )
        );

        return ScheduleListResponse {
            slack_view: slack_view,
            page: 0,
            total_pages: 0,
        };
    }

    let page_size_select = SlackBlockStaticSelectElement::new("page_size_select".into())
        .with_placeholder(SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Items per page".into())))
        .with_options(vec![
            SlackBlockChoiceItem::new(
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("5".into())),
                "5".into()
            ),
            SlackBlockChoiceItem::new(
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("10".into())),
                "10".into()
            ),
        ])
        .with_initial_option(SlackBlockChoiceItem::new(
            SlackBlockPlainTextOnly::from(SlackBlockPlainText::new(format!("{}", page_size))),
            format!("{}", page_size)
        ));

    blocks.push(
        SlackBlock::Section(
            SlackSectionBlock::new()
                .with_text(md!(format!("Showing {} - {} of {} schedules", start_idx + 1, end_idx, total_items)))
                .with_accessory(SlackSectionBlockElement::StaticSelect(page_size_select))
                ,
        )
    );

    blocks.push(SlackBlock::Divider(SlackDividerBlock::new()));

    // Schedule items
    for (idx, task) in page_tasks.iter().enumerate() {
        let blocks_for_task = build_schedule_item_blocks(task, idx);
        blocks.extend(blocks_for_task);
    }

    // Pagination controls
    if total_pages > 1 || true {  // Always show controls for refresh button
        let pagination_blocks = build_pagination_blocks(current_page, total_pages);
        blocks.extend(pagination_blocks);
    }

    let slack_view = SlackView::Modal(
        SlackModalView::new(SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Scheduled Tasks".into())), blocks)
    );

    ScheduleListResponse {
        slack_view,
        page: current_page,
        total_pages,
    }
}

fn build_schedule_item_blocks(task: &ScheduledTask, _idx: usize) -> Vec<SlackBlock> {
    let mut blocks = Vec::new();

    let last_updated_formatted = DateTime::parse_from_rfc3339(&task.last_updated_at)
        .ok()
        .and_then(|dt| {
            let tz: Result<Tz, _> = task.timezone.parse();
            tz.ok().map(|tz| dt.with_timezone(&tz).to_rfc3339_opts(SecondsFormat::Secs, true))
        })
        .unwrap_or_else(|| task.last_updated_at.clone());

    let text_content = format!(
        "*#{}* | *@{}*\nScheduled at: `{}` `{}`\nUpdated At: `{}`\nNext Run: `{}`",
        task.channel_name,
        task.user_group_handle,
        task.cron,
        task.timezone,
        last_updated_formatted,
        task.next_update_time,
    );

    let delete_button = SlackBlockButtonElement::new(
        format!("delete_schedule_{}", encode_schedule_id(task)).into(),
        SlackBlockPlainTextOnly::from(
            SlackBlockPlainText::new("Delete".into()).with_emoji(true)
        )
    )
    .with_style("danger".into())
    .with_confirm(
        SlackBlockConfirmItem::new(
            SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Delete Schedule?".into())),
            md!(format!(
                "Are you sure you want to delete the schedule for *#{}* / *@{}*?",
                task.channel_name, task.user_group_handle
            )),
            SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Delete".into())),
            SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Cancel".into()))
        )
        .with_style("danger".into())
    );

    // Main info section
    blocks.push(
        SlackBlock::Section(
            SlackSectionBlock::new()
                .with_text(md!(text_content))
                .with_accessory(SlackSectionBlockElement::Button(delete_button))
        )
    );

    // Divider
    blocks.push(SlackBlock::Divider(SlackDividerBlock::new()));

    blocks
}

/// Build pagination control blocks
fn build_pagination_blocks(current_page: usize, total_pages: usize) -> Vec<SlackBlock> {
    let mut blocks = Vec::new();

    let mut button_elements = Vec::new();

    // Refresh button
    button_elements.push(
        SlackActionBlockElement::Button(
            SlackBlockButtonElement::new(
                format!("refresh_page_{}", current_page).into(),
                SlackBlockPlainTextOnly::from(
                    SlackBlockPlainText::new("🔄 Refresh".into()).with_emoji(true)
                )
            )
        )
    );

    // Previous button
    if current_page > 0 {
        button_elements.push(
            SlackActionBlockElement::Button(
                SlackBlockButtonElement::new(
                    format!("page_{}", current_page - 1).into(),
                    SlackBlockPlainTextOnly::from(
                        SlackBlockPlainText::new("← Previous".into()).with_emoji(true)
                    )
                )
            )
        );
    }

    // Next button
    if current_page + 1 < total_pages {
        button_elements.push(
            SlackActionBlockElement::Button(
                SlackBlockButtonElement::new(
                    format!("page_{}", current_page + 1).into(),
                    SlackBlockPlainTextOnly::from(
                        SlackBlockPlainText::new("Next →".into()).with_emoji(true)
                    )
                )
            )
        );
    }

    // Actions block with buttons
    blocks.push(
        SlackBlock::Actions(SlackActionsBlock::new(button_elements))
    );

    blocks.push(
        SlackBlock::Context(
            SlackContextBlock::new(vec![
                SlackContextBlockElement::MarkDown(
                    SlackBlockMarkDownText::new(
                        format!("Loaded page {} of {}. Updated {}", current_page + 1, total_pages, current_time_markdown())
                    )
                )
            ])
        )
    );

    blocks
}

/// Generate a Slack markdown timestamp for the current time
/// https://docs.slack.dev/messaging/formatting-message-text/#date-formatting
fn current_time_markdown() -> String {
    let now = chrono::Utc::now();

    format!("<!date^{}^{{date_pretty}} {{time_secs}}|{}>", now.timestamp(), now.to_rfc3339())
}

/// Encode schedule identifiers into action_id payload
fn encode_schedule_id(task: &ScheduledTask) -> String {
    // Format: team_id:enterprise_id:task_id
    // We use base64 to handle any special characters
    let payload = format!("{}:{}:{}", task.team_id, task.enterprise_id, task.task_id);
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD.encode(payload.as_bytes())
}

/// Decode schedule identifiers from action_id payload
pub fn decode_schedule_id(encoded: &str) -> Option<(String, String, String)> {
    use base64::{Engine as _, engine::general_purpose};
    let decoded = general_purpose::STANDARD.decode(encoded.as_bytes()).ok()?;
    let payload = String::from_utf8(decoded).ok()?;
    let parts: Vec<&str> = payload.split(':').collect();

    if parts.len() != 3 {
        return None;
    }

    Some((
        parts[0].to_string(),
        parts[1].to_string(),
        parts[2].to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_encode_decode_schedule_id() {
        let task = create_test_task("general", "oncall");
        let encoded = encode_schedule_id(&task);
        let decoded = decode_schedule_id(&encoded).unwrap();

        assert_eq!(decoded.0, task.team_id);
        assert_eq!(decoded.1, task.enterprise_id);
        assert_eq!(decoded.2, task.task_id);
    }

    #[test]
    fn test_build_schedule_list_empty() {
        let tasks: Vec<ScheduledTask> = vec![];
        let response = build_schedule_list_blocks(&tasks, 0, 10);

        // Verify it's a Modal view
        match &response.slack_view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty(), "Blocks should have at least one item");
            }
            _ => panic!("Expected SlackView::Modal"),
        }

        assert_eq!(response.page, 0);
        assert_eq!(response.total_pages, 0);
    }

    #[test]
    fn test_build_schedule_list_single_page() {
        let tasks = vec![
            create_test_task("general", "oncall"),
            create_test_task("engineering", "eng-oncall"),
        ];
        let response = build_schedule_list_blocks(&tasks, 0, 10);

        // Verify it's a Modal view
        match &response.slack_view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty(), "Blocks should have at least one item");
            }
            _ => panic!("Expected SlackView::Modal"),
        }

        assert_eq!(response.page, 0);
        assert_eq!(response.total_pages, 1);
    }

    #[test]
    fn test_build_schedule_list_pagination() {
        // Create 25 tasks to test pagination (PAGE_SIZE = 10)
        let mut tasks = Vec::new();
        for i in 0..25 {
            tasks.push(create_test_task(&format!("channel{}", i), &format!("group{}", i)));
        }

        // Page 0
        let response = build_schedule_list_blocks(&tasks, 0, 10);

        // Verify it's a Modal view
        match &response.slack_view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty(), "Blocks should have at least one item");
            }
            _ => panic!("Expected SlackView::Modal"),
        }

        assert_eq!(response.page, 0);
        assert_eq!(response.total_pages, 3); // 25 tasks / 10 per page = 3 pages

        // Page 1
        let response = build_schedule_list_blocks(&tasks, 1, 10);

        // Verify it's a Modal view
        match &response.slack_view {
            SlackView::Modal(modal) => {
                assert!(!modal.blocks.is_empty(), "Blocks should have at least one item");
            }
            _ => panic!("Expected SlackView::Modal"),
        }

        assert_eq!(response.page, 1);
        assert_eq!(response.total_pages, 3);
    }
}
