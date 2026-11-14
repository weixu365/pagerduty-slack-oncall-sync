use crate::slack_handler::morphism_patches::interaction_event::SlackInteractionBlockActionsEvent;
use slack_morphism::prelude::*;

pub fn build_loading_modal() -> SlackView {
    SlackView::Modal(
        SlackModalView::new(
            "Create Schedule".into(),
            vec![SlackBlock::Section(
                SlackSectionBlock::new().with_text(pt!("⏳ Creating schedule...")),
            )],
        )
        .with_title("Create Schedule".into()),
    )
}

pub fn build_success_modal() -> SlackView {
    SlackView::Modal(
        SlackModalView::new(
            "Create Schedule".into(),
            vec![SlackBlock::Section(
                SlackSectionBlock::new().with_text(pt!("✅ Schedule created successfully!")),
            )],
        )
        .with_title("Create Schedule".into())
        .with_close("Close".into()),
    )
}

#[rustfmt::skip]
pub(crate) fn build_new_schedule_modal(on_call_text: Option<&str>, request: Option<&SlackInteractionBlockActionsEvent>, error: Option<&str>) -> SlackView {
    let default_oncall_text = "ℹ️ Current on-call user will be shown after you select a schedule";

    let mut blocks = vec![
        // PagerDuty Schedule
        SlackBlock::Section(SlackSectionBlock::new().with_text(pt!("PagerDuty Schedule"))),
        SlackBlock::Actions(
            SlackActionsBlock::new(vec![SlackActionBlockElement::ExternalSelect(
                SlackBlockExternalSelectElement {
                    action_id: Some("pagerduty_schedule_suggestion".into()),
                    placeholder: Some("Search for a PagerDuty schedule".into()),
                    initial_option: request
                        .and_then(|r| r.get_state("pagerduty_schedule_suggestion"))
                        .and_then(|state| state.selected_option.clone())
                        .map(|opt| {
                            SlackBlockChoiceItem::new(
                                SlackBlockPlainTextOnly::from(opt.text),
                                opt.value,
                            )
                        }),
                    min_query_length: Some(2),
                    confirm: None,
                    focus_on_load: None,
                }
            )]),
        ),

        // Current On-Call User (will be populated dynamically)
        SlackBlock::Section(
            SlackSectionBlock::new()
                .with_text(pt!(on_call_text.unwrap_or(default_oncall_text)))
                .with_block_id("pagerduty_oncall_info".into()),
        ),

        // Divider
        SlackBlock::Divider(SlackDividerBlock::new()),

        // Section 3: Slack Configuration Header
        SlackBlock::Section(SlackSectionBlock::new().with_text(pt!("Slack Configuration"))),

        // Channel Selector
        SlackBlock::Input(
            SlackInputBlock::new(
                "Channel".into(),
                SlackInputBlockElement::ConversationsSelect(
                    SlackBlockConversationsSelectElement::new()
                        .with_action_id("channel_value".into())
                        .with_default_to_current_conversation(true)
                        .opt_initial_conversation(
                            request
                                .and_then(|r| r.get_state("channel_value"))
                                .and_then(|state| state.selected_conversation.clone()),
                        )
                        .with_filter(
                            SlackBlockConversationFilter::new()
                                .with_include(vec![
                                    SlackConversationFilterInclude::Public,
                                    SlackConversationFilterInclude::Private,
                                ])
                                .with_exclude_external_shared_channels(true)
                                .with_exclude_bot_users(true),
                        )
                        .with_placeholder("Select a channel".into()),
                )
            )
        ),

        // User Group Selector (using external select to fetch from Slack)
        SlackBlock::Input(
            SlackInputBlock::new(
                "User Group".into(),
                SlackInputBlockElement::ExternalSelect(
                    SlackBlockExternalSelectElement {
                        action_id: Some("user_group_suggestion".into()),
                        placeholder: Some("Search for a user group".into()),
                        initial_option: request
                            .and_then(|r| r.get_state("user_group_suggestion"))
                            .and_then(|state| state.selected_option.clone())
                            .map(|opt| {
                                SlackBlockChoiceItem::new(
                                    SlackBlockPlainTextOnly::from(opt.text),
                                    opt.value,
                                )
                            }),
                        min_query_length: Some(2),
                        confirm: None,
                        focus_on_load: None,
                    }
                )
            )
        ),

        // Divider
        SlackBlock::Divider(SlackDividerBlock::new()),

        // Section 4: Schedule Configuration Header
        SlackBlock::Section(SlackSectionBlock::new().with_text(pt!("Schedule Configuration"))),

        // Cron Expression Input
        SlackBlock::Input(
            SlackInputBlock::new(
                "Cron Expression".into(),
                SlackInputBlockElement::PlainTextInput(
                    SlackBlockPlainTextInputElement::new().with_action_id("cron_value".into())
                        .with_initial_value(
                            request
                                .and_then(|r| r.get_state("cron_value"))
                                .and_then(|state| state.value)
                                .unwrap_or_else(|| "0 9 * * MON-FRI *".into())
                        )
                        .with_placeholder("e.g., 0 9 * * MON-FRI * (9 AM on weekdays)".into())
                )
            )
            .with_hint("Syntax: minutes hours day-of-month month day-of-week year".into())
        ),

        // Timezone Selector
        SlackBlock::Input(
            SlackInputBlock::new(
                "Timezone".into(),
                SlackInputBlockElement::ExternalSelect(
                    SlackBlockExternalSelectElement {
                        action_id: Some("timezone_suggestion".into()),
                        placeholder: Some("Search for a timezone".into()),
                        initial_option: Some(
                            request
                                .and_then(|r| r.get_state("timezone_suggestion"))
                                .and_then(|state| state.selected_option.clone())
                                .map(|opt| {
                                    SlackBlockChoiceItem::new(
                                        SlackBlockPlainTextOnly::from(opt.text),
                                        opt.value,
                                    )
                                })
                                .unwrap_or_else(|| {
                                    // Default to UTC if no previous selection
                                    SlackBlockChoiceItem::new("UTC".into(), "UTC".into())
                                })
                        ),
                        min_query_length: Some(2),
                        confirm: None,
                        focus_on_load: None,
                    }
                )
            )
        ),
    ];

    if let Some(error_msg) = error {
        blocks.push(SlackBlock::Section(
            SlackSectionBlock::new()
                .with_text(md!(format!(":warning: {}", error_msg)))
                .with_block_id("submission_error".into()),
        ));
    }

    SlackView::Modal(
        SlackModalView::new("Create Schedule".into(), blocks)
            .with_title("Create Schedule".into())
            .with_submit("Submit".into())
            .with_close("Cancel".into())
            .with_callback_id("new_schedule_form".into())
    )
}
