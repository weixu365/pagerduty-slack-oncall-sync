use rsb_derive::Builder;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, skip_serializing_none};
use slack_morphism::prelude::*;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SlackInputBlockElement {
    #[serde(rename = "static_select")]
    StaticSelect(SlackBlockStaticSelectElement),
    #[serde(rename = "multi_static_select")]
    MultiStaticSelect(SlackBlockMultiStaticSelectElement),
    #[serde(rename = "external_select")]
    ExternalSelect(SlackBlockExternalSelectElement),
    #[serde(rename = "multi_external_select")]
    MultiExternalSelect(SlackBlockMultiExternalSelectElement),
    #[serde(rename = "users_select")]
    UsersSelect(SlackBlockUsersSelectElement),
    #[serde(rename = "multi_users_select")]
    MultiUsersSelect(SlackBlockMultiUsersSelectElement),
    #[serde(rename = "multi_conversations_select")]
    MultiConversationsSelect(SlackBlockMultiConversationsSelectElement),
    #[serde(rename = "channels_select")]
    ChannelsSelect(SlackBlockChannelsSelectElement),
    #[serde(rename = "multi_channels_select")]
    MultiChannelsSelect(SlackBlockMultiChannelsSelectElement),
    #[serde(rename = "datepicker")]
    DatePicker(SlackBlockDatePickerElement),
    #[serde(rename = "timepicker")]
    TimePicker(SlackBlockTimePickerElement),
    #[serde(rename = "datetimepicker")]
    DateTimePicker(SlackBlockDateTimePickerElement),
    #[serde(rename = "plain_text_input")]
    PlainTextInput(SlackBlockPlainTextInputElement),
    #[serde(rename = "number_input")]
    NumberInput(SlackBlockNumberInputElement),
    #[serde(rename = "url_text_input")]
    UrlInput(SlackBlockUrlInputElement),
    #[serde(rename = "radio_buttons")]
    RadioButtons(SlackBlockRadioButtonsElement),
    #[serde(rename = "checkboxes")]
    Checkboxes(SlackBlockCheckboxesElement),
    #[serde(rename = "email_text_input")]
    EmailInput(SlackBlockEmailInputElement),

    // Override with custom select that supports conversation filters
    #[serde(rename = "conversations_select")]
    ConversationsSelect(SlackBlockConversationsSelectElement),
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Builder)]
pub struct SlackBlockConversationsSelectElement {
    pub action_id: SlackActionId,
    pub placeholder: Option<SlackBlockPlainTextOnly>,
    pub initial_conversation: Option<SlackConversationId>,
    pub default_to_current_conversation: Option<bool>,
    pub confirm: Option<SlackBlockConfirmItem>,
    pub response_url_enabled: Option<bool>,
    pub focus_on_load: Option<bool>,
    pub filter: Option<SlackConversationSelectFilter>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct SlackConversationSelectFilter {
    pub include: Option<Vec<String>>,
    pub exclude_external_shared_channels: Option<bool>,
    pub exclude_bot_users: Option<bool>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Builder)]
pub struct SlackInputBlock {
    pub block_id: Option<SlackBlockId>,
    pub label: SlackBlockPlainTextOnly,
    pub element: SlackInputBlockElement,
    pub hint: Option<SlackBlockPlainTextOnly>,
    pub optional: Option<bool>,
    pub dispatch_action: Option<bool>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SlackBlock {
    #[serde(rename = "section")]
    Section(SlackSectionBlock),
    #[serde(rename = "header")]
    Header(SlackHeaderBlock),
    #[serde(rename = "divider")]
    Divider(SlackDividerBlock),
    #[serde(rename = "image")]
    Image(SlackImageBlock),
    #[serde(rename = "actions")]
    Actions(SlackActionsBlock),
    #[serde(rename = "context")]
    Context(SlackContextBlock),
    #[serde(rename = "file")]
    File(SlackFileBlock),
    #[serde(rename = "video")]
    Video(SlackVideoBlock),
    #[serde(rename = "markdown")]
    Markdown(SlackMarkdownBlock),

    // This block is still undocumented, so we don't define any structure yet we can return it back,
    #[serde(rename = "rich_text")]
    RichText(serde_json::Value),
    #[serde(rename = "share_shortcut")]
    ShareShortcut(serde_json::Value),
    #[serde(rename = "event")]
    Event(serde_json::Value),

    // Override with custom SlackInputBlock
    #[serde(rename = "input")]
    Input(SlackInputBlock),
}

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Builder)]
pub struct SlackModalView {
    pub title: SlackBlockPlainTextOnly,
    pub blocks: Vec<SlackBlock>,
    pub close: Option<SlackBlockPlainTextOnly>,
    pub submit: Option<SlackBlockPlainTextOnly>,
    #[serde(default)]
    #[serde_as(as = "serde_with::NoneAsEmptyString")]
    pub private_metadata: Option<String>,
    #[serde(default)]
    #[serde_as(as = "serde_with::NoneAsEmptyString")]
    pub callback_id: Option<SlackCallbackId>,
    pub clear_on_close: Option<bool>,
    pub notify_on_close: Option<bool>,
    pub hash: Option<String>,
    #[serde(default)]
    #[serde_as(as = "serde_with::NoneAsEmptyString")]
    pub external_id: Option<String>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SlackView {
    #[serde(rename = "home")]
    Home(SlackHomeView),
    #[serde(rename = "modal")]
    Modal(SlackModalView),
}
