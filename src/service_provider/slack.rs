use std::sync::Arc;

use derive_more::Display;
use reqwest::{Client, Method};
use serde_derive::Deserialize;
use serde_json::{Error, Value, json};
use slack_morphism::blocks::{SlackBlock, SlackView};
use tracing::{error, info};

use crate::{errors::AppError, utils::base64::encode_with_pad};

#[derive(Deserialize, Debug)]
struct EmptyResponse;

#[derive(Deserialize, Debug)]
struct SlackResponse<T> {
    ok: bool,
    error: Option<String>,

    #[serde(flatten)]
    data: T,
}

#[derive(Deserialize, Debug)]
struct UserLookupResponse {
    user: Option<User>,
}

#[derive(Deserialize, Debug)]
struct ChannelResponse {
    channel: Option<Channel>,
}

#[derive(Deserialize, Debug)]
struct UserGroupUsersResponse {
    users: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
struct UserGroupsResponse {
    usergroups: Option<Vec<UserGroup>>,
}

#[derive(Deserialize, Debug, Display)]
#[display("Channel ({}, {}, {}, {})", name, is_channel, is_group, is_private)]
pub struct Channel {
    pub name: String,
    pub is_channel: bool,
    pub is_group: bool,
    pub is_private: bool,
}

#[derive(Deserialize, Debug, Display)]
#[display("User ({}, {})", id, name)]
pub struct User {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug, Display)]
#[display("UserGroup ({}, {}, {})", id, name, handle)]
pub struct UserGroup {
    pub id: String,
    pub name: String,
    pub description: String,
    pub handle: String,
}

pub struct Slack {
    http_client: Arc<Client>,
    api_token: String,
}

impl Slack {
    pub fn new(http_client: Arc<Client>, api_token: String) -> Slack {
        Slack { http_client, api_token }
    }

    pub async fn send_message(&self, channel_id: &str, message: &str) -> Result<(), AppError> {
        let payload = json!({
            "channel": channel_id,
            "text": message,
        });

        self.send_request::<_, ()>("chat.postMessage", Method::POST, None, Some(&payload))
            .await
    }

    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, AppError> {
        let params = json!({
            "email": email,
        });

        let response: UserLookupResponse = self
            .send_request("users.lookupByEmail", Method::GET, Some(&params), None)
            .await?;
        Ok(response.user)
    }

    pub async fn get_user_by_id(&self, id: &str) -> Result<Option<User>, AppError> {
        let params = json!({
            "user": id,
        });

        let response: UserLookupResponse = self
            .send_request("users.info", Method::GET, Some(&params), None)
            .await?;
        Ok(response.user)
    }

    pub async fn get_user_group(&self, name: &str) -> Result<UserGroup, AppError> {
        let user_groups = self.list_user_groups().await?;

        for user_group in user_groups {
            if user_group.name.eq(name) || user_group.handle.eq(name) {
                return Ok(user_group);
            }
        }

        Err(AppError::SlackUserGroupNotFoundError(name.to_string()))
    }

    pub async fn list_user_groups(&self) -> Result<Vec<UserGroup>, AppError> {
        let response: UserGroupsResponse = self
            .send_request::<_, ()>("usergroups.list", Method::GET, None, None)
            .await?;

        Ok(response.usergroups.unwrap_or_default())
    }

    pub async fn get_user_group_users(&self, user_group: &str) -> Result<Vec<String>, AppError> {
        let params = json!({
            "usergroup": user_group,
        });

        let response: UserGroupUsersResponse = self
            .send_request("usergroups.users.list", Method::GET, Some(&params), None)
            .await?;

        Ok(response.users.unwrap_or_default())
    }

    pub async fn update_user_group_users(&self, user_group: &str, users: &Vec<String>) -> Result<(), AppError> {
        let payload = json!({
            "usergroup": user_group,
            "users": users,
        });

        self.send_request::<EmptyResponse, ()>("usergroups.users.update", Method::POST, None, Some(&payload))
            .await?;

        Ok(())
    }

    pub async fn update_channel_topic(&self, channel_id: &str, topic: &str) -> Result<Option<Channel>, AppError> {
        let payload = json!({
            "channel": channel_id,
            "topic": topic,
        });

        let response: ChannelResponse = self
            .send_request::<_, ()>("conversations.setTopic", Method::POST, None, Some(&payload))
            .await?;

        Ok(response.channel)
    }

    async fn send_request<T, Q>(
        &self,
        endpoint: &str,
        method: Method,
        params: Option<&Q>,
        payload: Option<&Value>,
    ) -> Result<T, AppError>
    where
        T: for<'a> serde::Deserialize<'a>,
        Q: serde::Serialize,
    {
        let url = format!("https://slack.com/api/{}", endpoint);

        let mut request_builder = self
            .http_client
            .request(method.clone(), url)
            .bearer_auth(&self.api_token)
            .header("Content-Type", "application/json");

        if let Some(params) = params {
            request_builder = request_builder.query(params);
        }

        if let Some(payload) = payload {
            let body: String = payload.to_string();
            tracing::debug!(%method, endpoint, body, "Sending Slack request");
            request_builder = request_builder.body(body);
        }

        let response = request_builder.send().await?;

        if response.status().is_success() {
            let json_response: SlackResponse<T> = response.json().await?;

            if json_response.ok {
                tracing::debug!("Slack request finished successfully");
                Ok(json_response.data)
            } else if let Some(err) = json_response.error {
                tracing::error!(err, "Failed to call Slack API");
                Err(AppError::SlackError(err))
            } else {
                tracing::error!("SlackClient: Unknown error occurred");
                Err(AppError::SlackError("Unknown error".to_string()))
            }
        } else {
            tracing::error!(status = response.status().as_u16(), "Failed sending request to Slack");
            Err(AppError::SlackError(format!("Failed sending request to Slack, status: {}", response.status())))
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct SlackTeam {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct Enterprise {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct SlackUser {
    pub id: String,
}

#[derive(Deserialize, Debug)]
pub struct SlackOauthResponse {
    pub app_id: String,
    pub authed_user: SlackUser,

    pub scope: String,
    pub access_token: String,
    pub token_type: String,
    pub bot_user_id: String,
    pub team: SlackTeam,
    pub enterprise: Enterprise,
    pub is_enterprise_install: bool,
}

pub async fn swap_slack_access_token(
    http_client: &Client,
    temp_token: &str,
    slack_client_id: &str,
    slack_client_secret: &str,
) -> Result<SlackOauthResponse, AppError> {
    tracing::info!("Swap slack access token");
    let params = json!({
        "code": temp_token,
    });

    let auth = format!("Basic {}", encode_with_pad(format!("{}:{}", slack_client_id, slack_client_secret).as_bytes()));
    let response = http_client
        .request(Method::POST, "https://slack.com/api/oauth.v2.access")
        .header("Authorization", auth)
        .query(&params)
        .send()
        .await?;

    if response.status().is_success() {
        let response_body = response.text().await?;

        let json_response_result: Result<SlackResponse<SlackOauthResponse>, Error> =
            serde_json::from_str(&response_body);

        match json_response_result {
            Err(err) => {
                tracing::info!(response_body, "Failed to parse json response");
                Err(AppError::SlackError(err.to_string()))
            }
            Ok(json_response) => {
                if json_response.ok {
                    tracing::debug!("Slack request finished successfully");
                    Ok(json_response.data)
                } else if let Some(err) = json_response.error {
                    tracing::error!(err, "Failed to call Slack API");
                    Err(AppError::SlackError(err))
                } else {
                    tracing::error!("SlackClient: Unknown error occurred");
                    Err(AppError::SlackError("Unknown error".to_string()))
                }
            }
        }
    } else {
        tracing::error!(status = response.status().as_u16(), "Failed sending request to Slack");
        Err(AppError::SlackError(format!("Failed sending request to Slack, status: {}", response.status())))
    }
}

pub async fn send_slack_view(response_url: &str, slack_view: SlackView) -> Result<(), AppError> {
    let blocks = match slack_view {
        SlackView::Modal(modal) => modal.blocks,
        _ => return Err(AppError::InvalidData("Expected modal view".to_string())),
    };

    send_slack_blocks(response_url, &blocks).await
}

pub async fn send_slack_blocks(response_url: &str, blocks: &Vec<SlackBlock>) -> Result<(), AppError> {
    let response_payload = json!({
        "replace_original": true,
        "blocks": blocks,
    })
    .to_string();

    send_slack_message(response_url, response_payload).await
}

pub async fn send_slack_message(response_url: &str, response_payload: String) -> Result<(), AppError> {
    let payload_size = response_payload.len();

    info!(response_url, replace_original = true, payload_size = payload_size, "Posting message to Slack response_url");

    let client = reqwest::Client::new();
    match client
        .post(response_url)
        .header("Content-Type", "application/json")
        .body(response_payload.clone())
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read response body".to_string());

            if status.is_success() {
                info!(response_body = body, "Successfully sent interactive response to Slack");
            } else {
                error!(status = %status, response_body = body, response_payload, "Failed to send interactive response to Slack");
            }
        }
        Err(err) => {
            error!(%err, response_payload, "Error sending interactive response to Slack");
        }
    }

    Ok(())
}

pub async fn open_slack_modal(trigger_id: &str, modal: &SlackView, bot_access_token: &str) -> Result<(), AppError> {
    info!("Opening Slack modal");

    let modal_json =
        serde_json::to_value(modal).map_err(|e| AppError::InvalidData(format!("Failed to serialize modal: {}", e)))?;

    let payload = json!({
        "trigger_id": trigger_id,
        "view": modal_json
    });

    info!(payload=%payload, "Opening Slack modal");
    send_slack_request("https://slack.com/api/views.open", &payload, bot_access_token).await?;
    Ok(())
}

pub async fn update_slack_modal(
    view_id: &str,
    hash: &str,
    modal: &SlackView,
    bot_access_token: &str,
) -> Result<(), AppError> {
    info!("Updating Slack modal");

    let modal_json =
        serde_json::to_value(modal).map_err(|e| AppError::InvalidData(format!("Failed to serialize modal: {}", e)))?;

    let payload = json!({
        "view_id": view_id,
        "hash": hash,
        "view": modal_json,
    });

    send_slack_request("https://slack.com/api/views.update", &payload, bot_access_token).await?;
    Ok(())
}

pub async fn send_slack_request(url: &str, payload: &Value, bot_access_token: &str) -> Result<(), AppError> {
    info!(payload=%payload, "Sending Slack request to {}", url);

    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", bot_access_token))
        .json(&payload)
        .send()
        .await?;

    if response.status().is_success() {
        let response_body = response.text().await?;
        let json_response: Value = serde_json::from_str(&response_body)
            .map_err(|e| AppError::SlackError(format!("Failed to parse response: {}", e)))?;

        if json_response["ok"].as_bool().unwrap_or(false) {
            info!("Successfully sent Slack request");
            Ok(())
        } else {
            let error_msg = json_response["error"].as_str().unwrap_or("Unknown error");
            error!(error = error_msg, response = %response_body, "Failed to send Slack request");
            Err(AppError::SlackError(format!("Failed to send Slack request: {}", error_msg)))
        }
    } else {
        error!(status = response.status().as_u16(), "Failed to send Slack request");
        Err(AppError::SlackError(format!("HTTP error: {}", response.status())))
    }
}
