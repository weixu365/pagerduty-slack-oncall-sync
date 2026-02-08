use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use slack_morphism::blocks::{SlackActionState, SlackStatefulView, SlackViewStateValue};
use slack_morphism::events::{
    SlackInteractionActionContainer, SlackInteractionActionInfo, SlackInteractionBlockSuggestionEvent,
    SlackInteractionDialogueSubmissionEvent, SlackInteractionMessageActionEvent, SlackInteractionShortcutEvent,
    SlackInteractionViewClosedEvent,
};
use slack_morphism::{
    SlackAppId, SlackBasicChannelInfo, SlackBasicUserInfo, SlackHistoryMessage, SlackResponseUrl, SlackTeamId, SlackTriggerId
};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SlackInteractionEvent {
    #[serde(rename = "block_actions")]
    BlockActions(SlackInteractionBlockActionsEvent),
    #[serde(rename = "block_suggestion")]
    BlockSuggestion(SlackInteractionBlockSuggestionEvent),
    #[serde(rename = "dialog_submission")]
    DialogSubmission(SlackInteractionDialogueSubmissionEvent),
    #[serde(rename = "message_action")]
    MessageAction(SlackInteractionMessageActionEvent),
    #[serde(rename = "shortcut")]
    Shortcut(SlackInteractionShortcutEvent),
    #[serde(rename = "view_submission")]
    ViewSubmission(SlackInteractionViewSubmissionEvent),
    #[serde(rename = "view_closed")]
    ViewClosed(SlackInteractionViewClosedEvent),
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct SlackInteractionBlockActionsEvent {
    pub team: SlackTeamInfo,
    pub user: Option<SlackBasicUserInfo>,
    pub api_app_id: SlackAppId,
    pub container: SlackInteractionActionContainer,
    pub trigger_id: SlackTriggerId,
    pub channel: Option<SlackBasicChannelInfo>,
    pub message: Option<SlackHistoryMessage>,
    pub view: Option<SlackStatefulView>,
    pub response_url: Option<SlackResponseUrl>,
    pub actions: Option<Vec<SlackInteractionActionInfo>>,
    pub state: Option<SlackActionState>,
}

impl SlackInteractionBlockActionsEvent {
    pub fn get_state(&self, action_id: &str) -> Option<SlackViewStateValue> {
        let state = self.view.as_ref()?.state_params.state.as_ref()?;
        let slack_action_id = action_id.into();

        for block_states in state.values.values() {
            if let Some(action_state) = block_states.get(&slack_action_id) {
                return Some(action_state.clone());
            }
        }

        None
    }
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct SlackInteractionViewSubmissionEvent {
    pub team: SlackTeamInfo,
    pub user: SlackBasicUserInfo,
    pub view: SlackStatefulView,
    pub trigger_id: Option<SlackTriggerId>,
    pub is_enterprise_install: bool,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct SlackTeamInfo {
    pub id: SlackTeamId,
    pub name: Option<String>,
    pub domain: Option<String>,

    pub enterprise_id: Option<String>,
    pub enterprise_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use crate::errors::AppError;

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_slack_command_request() -> Result<(), AppError> {
        let payload_json = {
            r#"
            {
                "type": "block_actions",
                "user": {
                    "id": "USER_ID",
                    "username": "USER_NAME",
                    "name": "USER_NAME",
                    "team_id": "TEAM_ID"
                },
                "api_app_id": "APP_ID",
                "token": "TOKEN",
                "container": {
                    "type": "view",
                    "view_id": "VIEW_ID"
                },
                "trigger_id": "10478104431457.2785258569.TRIGGER_ID",
                "team": {
                    "id": "TEAM_ID",
                    "domain": "DOMAIN",
                    "enterprise_id": "ENTERPRISE_ID",
                    "enterprise_name": "ENTERPRISE_NAME"
                },
                "enterprise": {
                    "id": "ENTERPRISE_ID",
                    "name": "ENTERPRISE_NAME"
                },
                "is_enterprise_install": false,
                "view": {
                    "id": "VIEW_ID",
                    "team_id": "TEAM_ID",
                    "type": "modal",
                    "blocks": [
                        {
                            "type": "section",
                            "block_id": "ONGpz",
                            "text": {
                                "type": "plain_text",
                                "text": "PagerDuty+Schedule",
                                "emoji": true
                            }
                        },
                        {
                            "type": "actions",
                            "block_id": "BQiXA",
                            "elements": [
                                {
                                    "type": "external_select",
                                    "action_id": "pagerduty_schedule_suggestion",
                                    "placeholder": {
                                        "type": "plain_text",
                                        "text": "Search+for+a+PagerDuty+schedule",
                                        "emoji": true
                                    },
                                    "min_query_length": 2
                                }
                            ]
                        },
                        {
                            "type": "section",
                            "block_id": "pagerduty_oncall_info",
                            "text": {
                                "type": "plain_text",
                                "text": ":information_source:+Current+on-call:+ONCALL_NAME+&lt;ONCALL_EMAIL&gt;",
                                "emoji": true
                            }
                        },
                        {
                            "type": "divider",
                            "block_id": "n/61e"
                        },
                        {
                            "type": "section",
                            "block_id": "kTGZS",
                            "text": {
                                "type": "plain_text",
                                "text": "Slack+Configuration",
                                "emoji": true
                            }
                        },
                        {
                            "type": "input",
                            "block_id": "sEPDi",
                            "label": {
                                "type": "plain_text",
                                "text": "Channel",
                                "emoji": true
                            },
                            "optional": false,
                            "dispatch_action": false,
                            "element": {
                                "type": "channels_select",
                                "action_id": "channel_value",
                                "placeholder": {
                                    "type": "plain_text",
                                    "text": "Select+a+channel",
                                    "emoji": true
                                }
                            }
                        },
                        {
                            "type": "input",
                            "block_id": "ifiK2",
                            "label": {
                                "type": "plain_text",
                                "text": "User+Group",
                                "emoji": true
                            },
                            "optional": false,
                            "dispatch_action": false,
                            "element": {
                                "type": "external_select",
                                "action_id": "user_group_suggestion",
                                "placeholder": {
                                    "type": "plain_text",
                                    "text": "Search+for+a+user+group",
                                    "emoji": true
                                },
                                "min_query_length": 2
                            }
                        },
                        {
                            "type": "divider",
                            "block_id": "6yjIU"
                        },
                        {
                            "type": "section",
                            "block_id": "cM+LM",
                            "text": {
                                "type": "plain_text",
                                "text": "Schedule+Configuration",
                                "emoji": true
                            }
                        },
                        {
                            "type": "input",
                            "block_id": "qw81U",
                            "label": {
                                "type": "plain_text",
                                "text": "Cron+Expression",
                                "emoji": true
                            },
                            "hint": {
                                "type": "plain_text",
                                "text": "Create+your+own+using+cron+builder:+[https://crontab.cronhub.io/]()",
                                "emoji": true
                            },
                            "optional": false,
                            "dispatch_action": false,
                            "element": {
                                "type": "plain_text_input",
                                "action_id": "cron_value",
                                "placeholder": {
                                    "type": "plain_text",
                                    "text": "e.g.,+0+9+*+*+MON-FRI+(9+AM+on+weekdays)",
                                    "emoji": true
                                },
                                "initial_value": "0+9+*+*+MON-FRI",
                                "dispatch_action_config": {
                                    "trigger_actions_on": [
                                        "on_enter_pressed"
                                    ]
                                }
                            }
                        },
                        {
                            "type": "input",
                            "block_id": "N01NF",
                            "label": {
                                "type": "plain_text",
                                "text": "Timezone",
                                "emoji": true
                            },
                            "optional": false,
                            "dispatch_action": false,
                            "element": {
                                "type": "external_select",
                                "action_id": "timezone_suggestion",
                                "initial_option": {
                                    "text": {
                                        "type": "plain_text",
                                        "text": "UTC",
                                        "emoji": true
                                    },
                                    "value": "UTC"
                                },
                                "placeholder": {
                                    "type": "plain_text",
                                    "text": "Search+for+a+timezone",
                                    "emoji": true
                                },
                                "min_query_length": 2
                            }
                        }
                    ],
                    "private_metadata": "",
                    "callback_id": "new_schedule_form",
                    "state": {
                        "values": {
                            "BQiXA": {
                                "pagerduty_schedule_suggestion": {
                                    "type": "external_select",
                                    "selected_option": {
                                        "text": {
                                            "type": "plain_text",
                                            "text": "SCHEDULE_NAME+(SCHEDULE_ID)",
                                            "emoji": true
                                        },
                                        "value": "SCHEDULE_ID"
                                    }
                                }
                            },
                            "sEPDi": {
                                "channel_value": {
                                    "type": "channels_select",
                                    "selected_channel": "SELECTED_CHANNEL_ID"
                                }
                            },
                            "ifiK2": {
                                "user_group_suggestion": {
                                    "type": "external_select",
                                    "selected_option": {
                                        "text": {
                                            "type": "plain_text",
                                            "text": "@team-support+(TEAM+Support)",
                                            "emoji": true
                                        },
                                        "value": "<!subteam^TEAM_ID|@team-support>"
                                    }
                                }
                            },
                            "qw81U": {
                                "cron_value": {
                                    "type": "plain_text_input",
                                    "value": "0+9+*+*+MON-FRI"
                                }
                            },
                            "N01NF": {
                                "timezone_suggestion": {
                                    "type": "external_select",
                                    "selected_option": {
                                        "text": {
                                            "type": "plain_text",
                                            "text": "Australia/Melbourne+(UTC+11:00)",
                                            "emoji": true
                                        },
                                        "value": "Australia/Melbourne"
                                    }
                                }
                            }
                        }
                    },
                    "hash": "1770510235.wPmahZOz",
                    "title": {
                        "type": "plain_text",
                        "text": "Create+Schedule",
                        "emoji": true
                    },
                    "clear_on_close": false,
                    "notify_on_close": false,
                    "close": {
                        "type": "plain_text",
                        "text": "Cancel",
                        "emoji": true
                    },
                    "submit": {
                        "type": "plain_text",
                        "text": "Submit",
                        "emoji": true
                    },
                    "previous_view_id": null,
                    "root_view_id": "ROOT_VIEW_ID",
                    "app_id": "APP_ID",
                    "external_id": "",
                    "app_installed_team_id": "TEAM_ID",
                    "bot_id": "BOT_ID"
                },
                "actions": [
                    {
                        "type": "external_select",
                        "action_id": "pagerduty_schedule_suggestion",
                        "block_id": "BQiXA",
                        "selected_option": {
                            "text": {
                                "type": "plain_text",
                                "text": "SCHEDULE_NAME+(SCHEDULE_ID)",
                                "emoji": true
                            },
                            "value": "SCHEDULE_ID"
                        },
                        "placeholder": {
                            "type": "plain_text",
                            "text": "Search+for+a+PagerDuty+schedule",
                            "emoji": true
                        },
                        "action_ts": "1770515157.007365"
                    }
                ]
            }
            "#
        };

        let request: SlackInteractionEvent = serde_json::from_str(&payload_json)
            .map_err(|e| AppError::InvalidData(format!("Failed to parse payload JSON: {}", e)))?;

        match request {
            SlackInteractionEvent::BlockActions(event) => {
                assert_eq!(event.user.as_ref().map(|u| u.id.0.as_str()), Some("USER_ID"));
                assert_eq!(event.team.id.0.as_str(), "TEAM_ID");
                assert_eq!(event.actions.as_ref().map(|a| a.len()), Some(1));
                assert_eq!(
                    event
                        .actions
                        .as_ref()
                        .and_then(|a| a.first())
                        .map(|a| a.action_id.0.as_str()),
                    Some("pagerduty_schedule_suggestion"),
                );
            }
            _ => panic!("Expected BlockActions event"),
        }
        Ok(())
    }
}
