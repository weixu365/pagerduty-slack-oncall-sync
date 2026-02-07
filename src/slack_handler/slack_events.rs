use serde::{Deserialize, Serialize};
use slack_morphism::blocks::{SlackActionState, SlackStatefulView};
use slack_morphism::events::{
    SlackInteractionActionContainer, SlackInteractionActionInfo, SlackInteractionBlockSuggestionEvent, SlackInteractionDialogueSubmissionEvent, SlackInteractionMessageActionEvent, SlackInteractionShortcutEvent, SlackInteractionViewClosedEvent, SlackInteractionViewSubmissionEvent
};
use slack_morphism::{SlackAppId, SlackBasicChannelInfo, SlackBasicUserInfo, SlackHistoryMessage, SlackResponseUrl, SlackTeamId, SlackTriggerId};
use serde_with::{skip_serializing_none};


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

#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct SlackTeamInfo {
    pub id: SlackTeamId,
    pub name: Option<String>,
    pub domain: Option<String>,
    
    pub enterprise_id: Option<String>,
    pub enterprise_name: Option<String>,
}
