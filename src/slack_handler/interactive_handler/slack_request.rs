use crate::errors::AppError;
use crate::slack_handler::slack_events::SlackInteractionEvent;
use crate::slack_handler::views::schedule_list::ScheduleFilter;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
pub struct InteractiveRequest {
    pub user: InteractiveUser,
    pub team: InteractiveTeam,
    pub channel: Option<InteractiveChannel>,
    pub enterprise: Option<InteractiveEnterprise>,
    pub actions: Vec<BlockAction>,
    pub view: Option<ModalView>,
    pub response_url: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct ModalView {
    pub id: String,
    pub hash: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct InteractiveUser {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct InteractiveTeam {
    pub id: String,
    pub domain: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct InteractiveChannel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct InteractiveEnterprise {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct BlockAction {
    pub action_id: String,
    pub block_id: Option<String>,
    pub value: Option<String>,
    pub selected_option: Option<SelectedOption>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct SelectedOption {
    pub value: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct DeleteScheduleValue {
    pub team_id: String,
    pub enterprise_id: String,
    pub task_id: String,
    pub page: usize,
    pub page_size: usize,
    pub filter: ScheduleFilter,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct PaginationValue {
    pub page: usize,
    pub page_size: usize,
    pub filter: ScheduleFilter,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct PageSizeChangeValue {
    pub page_size: usize,
    pub filter: ScheduleFilter,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct FilterChangeValue {
    pub filter: ScheduleFilter,
    pub page_size: usize,
}

#[derive(Debug, Deserialize)]
pub struct SlackInteractiveActionRequest {
    pub payload: String,
}

pub fn parse_slack_request(request_body: &str) -> Result<SlackInteractionEvent, AppError> {
    let params: SlackInteractiveActionRequest = serde_urlencoded::from_str(request_body)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse request body: {}", e)))?;

    let request: SlackInteractionEvent = serde_json::from_str(&params.payload)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse payload JSON: {}", e)))?;

    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_slack_command_request_valid_full() -> Result<(), AppError> {
        let request_body = "payload=%7B%22type%22%3A%22block_actions%22%2C%22user%22%3A%7B%22id%22%3A%22USER_ID%22%2C%22username%22%3A%22nnn%22%2C%22name%22%3A%22nnn%22%2C%22team_id%22%3A%22ddd%22%7D%2C%22api_app_id%22%3A%22123456%22%2C%22token%22%3A%22xxxxxx%22%2C%22container%22%3A%7B%22type%22%3A%22message%22%2C%22message_ts%22%3A%221769328187.002300%22%2C%22channel_id%22%3A%22C0000001%22%2C%22is_ephemeral%22%3Atrue%7D%2C%22trigger_id%22%3A%2200000.1111.222222222%22%2C%22team%22%3A%7B%22id%22%3A%22ddd%22%2C%22domain%22%3A%22seekchat%22%2C%22enterprise_id%22%3A%22aaa%22%2C%22enterprise_name%22%3A%22bbb%22%7D%2C%22enterprise%22%3A%7B%22id%22%3A%22aaa%22%2C%22name%22%3A%22bbb%22%7D%2C%22is_enterprise_install%22%3Afalse%2C%22channel%22%3A%7B%22id%22%3A%22C0000001%22%2C%22name%22%3A%22privategroup%22%7D%2C%22state%22%3A%7B%22values%22%3A%7B%7D%7D%2C%22response_url%22%3A%22https%3A%2F%2Fhooks.slack.com%2Factions%2Fddd%2Fabcabc%2Fdefdef%22%2C%22actions%22%3A%5B%7B%22action_id%22%3A%22refresh_page_0%22%2C%22block_id%22%3A%223Ahe0%22%2C%22text%22%3A%7B%22type%22%3A%22plain_text%22%2C%22text%22%3A%22%3Aarrows_counterclockwise%3A%2BRefresh%22%2C%22emoji%22%3Atrue%7D%2C%22type%22%3A%22button%22%2C%22action_ts%22%3A%221769384057.506439%22%7D%5D%7D";

        let request = parse_slack_request(request_body)?;

        // Verify that we successfully parsed a SlackInteractionEvent::BlockActions
        match request {
            SlackInteractionEvent::BlockActions(event) => {
                assert_eq!(event.user.as_ref().map(|u| u.id.0.as_str()), Some("USER_ID"));
                assert_eq!(event.team.id.0.as_str(), "ddd");
                assert_eq!(event.channel.as_ref().map(|c| c.id.0.as_str()), Some("C0000001"));
                assert_eq!(
                    event.response_url.as_ref().map(|u| u.0.as_str()),
                    Some("https://hooks.slack.com/actions/ddd/abcabc/defdef")
                );
                assert_eq!(event.actions.as_ref().map(|a| a.len()), Some(1));
                assert_eq!(
                    event
                        .actions
                        .as_ref()
                        .and_then(|a| a.first())
                        .map(|a| a.action_id.0.as_str()),
                    Some("refresh_page_0")
                );
            }
            _ => panic!("Expected BlockActions event"),
        }
        Ok(())
    }

    #[test]
    fn test_parse_slack_command_request_invalid_format() -> Result<(), AppError> {
        let request_body = "not a valid urlencoded string!!!";

        let result = parse_slack_request(request_body);
        // This should actually succeed because serde_urlencoded is lenient
        // It will treat the whole string as a key with no value
        assert!(result.is_err());
        Ok(())
    }
}
