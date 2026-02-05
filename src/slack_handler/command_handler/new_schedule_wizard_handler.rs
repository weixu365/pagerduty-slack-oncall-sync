use crate::{
    db::SlackInstallationRepository,
    errors::AppError,
    service_provider::slack::open_slack_modal,
    slack_handler::command_handler::slack_request::SlackCommandRequest,
};
use slack_morphism::prelude::*;

const DEFAULT_ONCALL_TEXT: &str =
    "ℹ️ Current on-call user will be shown after you select a schedule";

/// Build the wizard modal for creating a new schedule
pub async fn handle_new_schedule_wizard(
    params: &SlackCommandRequest,
    trigger_id: &str,
    slack_installations_db: &dyn SlackInstallationRepository,
) -> Result<(), AppError> {
    tracing::info!(user_id = %params.user_id, "Opening new schedule wizard");

    // Check if PagerDuty token is configured
    let installation = slack_installations_db
        .get_slack_installation(&params.team_id, &params.enterprise_id)
        .await?;

    if installation.pager_duty_token.is_none() {
        return Err(AppError::InvalidData(
            "PagerDuty API token not configured. Please run `/oncall setup-pagerduty --pagerduty-api-key YOUR_KEY` first.".to_string(),
        ));
    }

    let modal = build_new_schedule_modal();
    let bot_access_token = &installation.access_token;
    open_slack_modal(trigger_id, &modal, bot_access_token).await?;

    Ok(())
}

pub(crate) fn build_new_schedule_modal() -> SlackView {
    build_new_schedule_modal_with_oncall(DEFAULT_ONCALL_TEXT)
}

pub(crate) fn build_new_schedule_modal_with_oncall(on_call_text: &str) -> SlackView {
    let blocks = vec![
        // Section 1: PagerDuty Schedule
        SlackBlock::Section(
            SlackSectionBlock::new()
                .with_text(SlackBlockText::from(
                    SlackBlockPlainText::new("PagerDuty Schedule".into()).with_emoji(true),
                )),
        ),
        SlackBlock::Actions(
            SlackActionsBlock::new(vec![SlackActionBlockElement::ExternalSelect(
                SlackBlockExternalSelectElement::new("pagerduty_schedule_suggestion".into())
                    .with_placeholder(SlackBlockPlainTextOnly::from(
                        SlackBlockPlainText::new("Search for a PagerDuty schedule".into()),
                    ))
                    .with_min_query_length(2),
            )]),
        ),

        // Section 2: Current On-Call User (will be populated dynamically)
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
                    SlackBlockChannelsSelectElement::new(
                        "channel_value".into(),
                    )
                    .with_placeholder(SlackBlockPlainTextOnly::from(
                        SlackBlockPlainText::new("Select a channel".into())
                    ))
                )
            )
        ),

        // User Group Selector (using external select to fetch from Slack)
        SlackBlock::Input(
            SlackInputBlock::new(
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("User Group".into())),
                SlackInputBlockElement::ExternalSelect(
                    SlackBlockExternalSelectElement::new("user_group_suggestion".into())
                        .with_placeholder(SlackBlockPlainTextOnly::from(
                            SlackBlockPlainText::new("Search for a user group".into())
                        ))
                        .with_min_query_length(2)
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
                        .with_initial_value("0 9 * * MON-FRI".into())
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
                    SlackBlockExternalSelectElement::new("timezone_suggestion".into())
                        .with_placeholder(SlackBlockPlainTextOnly::from(
                            SlackBlockPlainText::new("Search for a timezone".into())
                        ))
                        .with_min_query_length(2)
                        .with_initial_option(SlackBlockChoiceItem::new(
                            SlackBlockPlainTextOnly::from(SlackBlockPlainText::new("UTC".into())),
                            "UTC".into(),
                        ))
                ),
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
        .with_callback_id("new_schedule_submit".into())
    )
}
