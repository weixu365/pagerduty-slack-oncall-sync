use rsb_derive::Builder;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use slack_morphism::prelude::*;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SlackPushEvent {
    #[serde(rename = "url_verification")]
    UrlVerification(SlackUrlVerificationEvent),
    #[serde(rename = "event_callback")]
    EventCallback(SlackPushEventCallback),
    #[serde(rename = "app_rate_limited")]
    AppRateLimited(SlackAppRateLimitedEvent),
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Builder)]
pub struct SlackPushEventCallback {
    pub team_id: SlackTeamId,
    pub enterprise_id: Option<SlackEnterpriseId>,
    pub api_app_id: SlackAppId,
    pub event: SlackEventCallbackBody,
    pub event_id: SlackEventId,
    pub event_time: SlackDateTime,
    pub event_context: Option<SlackEventContext>,
    pub authed_users: Option<Vec<SlackUserId>>,
    pub authorizations: Option<Vec<SlackEventAuthorization>>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SlackEventCallbackBody {
    Message(SlackMessageEvent),
    AppHomeOpened(SlackAppHomeOpenedEvent),
    AppMention(SlackAppMentionEvent),
    AppUninstalled(SlackAppUninstalledEvent),
    LinkShared(SlackLinkSharedEvent),
    EmojiChanged(SlackEmojiChangedEvent),
    MemberJoinedChannel(SlackMemberJoinedChannelEvent),
    MemberLeftChannel(SlackMemberLeftChannelEvent),
    ChannelCreated(SlackChannelCreatedEvent),
    ChannelDeleted(SlackChannelDeletedEvent),
    ChannelArchive(SlackChannelArchiveEvent),
    ChannelRename(SlackChannelRenameEvent),
    ChannelUnarchive(SlackChannelUnarchiveEvent),
    TeamJoin(SlackTeamJoinEvent),
    FileCreated(SlackFileCreatedEvent),
    FileChange(SlackFileChangedEvent),
    FileDeleted(SlackFileDeletedEvent),
    FileShared(SlackFileSharedEvent),
    FileUnshared(SlackFileUnsharedEvent),
    FilePublic(SlackFilePublicEvent),
    ReactionAdded(SlackReactionAddedEvent),
    ReactionRemoved(SlackReactionRemovedEvent),
    StarAdded(SlackStarAddedEvent),
    StarRemoved(SlackStarRemovedEvent),
    UserChange(SlackUserChangeEvent),
    UserStatusChanged(SlackUserStatusChangedEvent),
    AssistantThreadStarted(SlackAssistantThreadStartedEvent),
    AssistantThreadContextChanged(SlackAssistantThreadContextChangedEvent),
}


#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Builder)]
pub struct SlackAppHomeOpenedEvent {
    pub user: SlackUserId,
    pub channel: SlackChannelId,
    pub tab: Option<String>,
    pub view: Option<SlackView>,
}
