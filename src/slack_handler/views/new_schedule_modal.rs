use slack_morphism::prelude::*;
use crate::slack_handler::slack_events::SlackInteractionBlockActionsEvent;

#[rustfmt::skip]
pub(crate) fn build_new_schedule_modal_with_oncall(on_call_text: &str, request: Option<&SlackInteractionBlockActionsEvent>) -> SlackView {
    let blocks = vec![
        // PagerDuty Schedule
        SlackBlock::Section(
            SlackSectionBlock::new()
                .with_text(SlackBlockText::from(
                    SlackBlockPlainText::new("PagerDuty Schedule".into()).with_emoji(true),
                )),
        ),
        SlackBlock::Actions(
            SlackActionsBlock::new(vec![SlackActionBlockElement::ExternalSelect(
                SlackBlockExternalSelectElement {
                    action_id: "pagerduty_schedule_suggestion".into(),
                    placeholder: Some(SlackBlockPlainTextOnly::from(
                        SlackBlockPlainText::new("Search for a PagerDuty schedule".into()),
                    )),
                    initial_option: request
                        .and_then(|r| r.get_state("pagerduty_schedule_suggestion"))
                        .and_then(|state| state.selected_option.clone())
                        .map(|opt| {
                            SlackBlockChoiceItem::new(
                                SlackBlockPlainTextOnly::from(opt.text),
                                opt.value.into(),
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
                .with_text(SlackBlockText::from(
                    SlackBlockPlainText::new(on_call_text.into()).with_emoji(true),
                ))
                .with_block_id("pagerduty_oncall_info".into()),
        ),

        // Divider
        SlackBlock::Divider(SlackDividerBlock::new()),

        // Section 3: Slack Configuration Header
        SlackBlock::Section(
            SlackSectionBlock::new()
                .with_text(SlackBlockText::from(
                    SlackBlockPlainText::new("Slack Configuration".into()).with_emoji(true),
                ))
        ),

        // Channel Selector
        SlackBlock::Input(
            SlackInputBlock::new(
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Channel".into())),
                SlackInputBlockElement::ChannelsSelect(
                    SlackBlockChannelsSelectElement {
                        action_id: "channel_value".into(),
                        initial_channel: request
                            .and_then(|r| r.get_state("channel_value"))
                            .and_then(|state| state.selected_channel.clone()),
                        confirm: None,
                        response_url_enabled: None,
                        focus_on_load: None,
                        placeholder: Some(SlackBlockPlainTextOnly::from(
                            SlackBlockPlainText::new("Select a channel".into())
                        )),
                    }
                )
            )
        ),

        // User Group Selector (using external select to fetch from Slack)
        SlackBlock::Input(
            SlackInputBlock::new(
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("User Group".into())),
                SlackInputBlockElement::ExternalSelect(
                    SlackBlockExternalSelectElement {
                        action_id: "user_group_suggestion".into(),
                        placeholder: Some(SlackBlockPlainTextOnly::from(
                            SlackBlockPlainText::new("Search for a user group".into())
                        )),
                        initial_option: request
                            .and_then(|r| r.get_state("user_group_suggestion"))
                            .and_then(|state| state.selected_option.clone())
                            .map(|opt| {
                                SlackBlockChoiceItem::new(
                                    SlackBlockPlainTextOnly::from(opt.text),
                                    opt.value.into(),
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
        SlackBlock::Section(
            SlackSectionBlock::new()
                .with_text(SlackBlockText::from(
                    SlackBlockPlainText::new("Schedule Configuration".into()).with_emoji(true),
                ))
        ),

        // Cron Expression Input
        SlackBlock::Input(
            SlackInputBlock::new(
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Cron Expression".into())),
                SlackInputBlockElement::PlainTextInput(
                    SlackBlockPlainTextInputElement::new("cron_value".into())
                        .with_initial_value(
                            request
                                .and_then(|r| r.get_state("cron_value"))
                                .and_then(|state| state.value)
                                .unwrap_or_else(|| "0 9 * * MON-FRI".into())
                        )
                        .with_placeholder(SlackBlockPlainTextOnly::from(
                            SlackBlockPlainText::new("e.g., 0 9 * * MON-FRI (9 AM on weekdays)".into())
                        ))
                )
            )
            .with_hint(SlackBlockPlainTextOnly::from(
                SlackBlockPlainText::new("Create your own using cron builder: [https://crontab.cronhub.io/]()".into())
            ))
        ),

        // Timezone Selector
        SlackBlock::Input(
            SlackInputBlock::new(
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Timezone".into())),
                SlackInputBlockElement::ExternalSelect(
                    SlackBlockExternalSelectElement {
                        action_id: "timezone_suggestion".into(),
                        placeholder: Some(SlackBlockPlainTextOnly::from(
                            SlackBlockPlainText::new("Search for a timezone".into())
                        )),
                        initial_option: Some(
                            request
                                .and_then(|r| r.get_state("timezone_suggestion"))
                                .and_then(|state| state.selected_option.clone())
                                .map(|opt| {
                                    SlackBlockChoiceItem::new(
                                        SlackBlockPlainTextOnly::from(opt.text),
                                        opt.value.into(),
                                    )
                                })
                                .unwrap_or_else(|| {
                                    // Default to UTC if no previous selection
                                    SlackBlockChoiceItem::new(
                                        SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("UTC".into())),
                                        "UTC".into(),
                                    )
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

    SlackView::Modal(
        SlackModalView::new(
            SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("Create Schedule".into())),
            blocks,
        )
        .with_title(SlackBlockPlainTextOnly::from("Create Schedule"))
        .with_submit(SlackBlockPlainTextOnly::from("Submit"))
        .with_close(SlackBlockPlainTextOnly::from("Cancel"))
        .with_callback_id("new_schedule_form".into())
    )
}
