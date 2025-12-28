use std::sync::Arc;

use derive_more::Display;
use reqwest::{Method, Client};
use serde_derive::Deserialize;
use serde_json::{json, Value, Error};

use crate::{errors::AppError, base64::encode_with_pad};


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

#[derive(Deserialize, Debug)]
struct PostMessageResponse {
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
    http_client: Arc<Box<Client>>,
    api_token: String,
}

impl Slack {
    pub fn new(http_client: Arc<Box<Client>>, api_token: String) -> Slack {
        Slack{ http_client, api_token}
    }
    
    pub async fn send_message(&self, channel_id: &str, message: &str) -> Result<(), AppError> {
        let payload = json!({
            "channel": channel_id,
            "text": message,
        });

        self.send_request::<_, ()>("chat.postMessage", Method::POST, None, Some(&payload)).await
    }
    
    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, AppError> {
        let params = json!({
            "email": email,
        });

        let response: UserLookupResponse = self.send_request("users.lookupByEmail", Method::GET, Some(&params), None).await?;
        Ok(response.user)
    }
    
    pub async fn get_user_by_id(&self, id: &str) -> Result<Option<User>, AppError> {
        let params = json!({
            "user": id,
        });

        let response: UserLookupResponse = self.send_request("users.info", Method::GET, Some(&params), None).await?;
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
        let response: UserGroupsResponse = self.send_request::<_, ()>("usergroups.list", Method::GET, None, None).await?;

        Ok(response.usergroups.unwrap_or_default())
    }

    pub async fn get_user_group_users(&self, user_group: &str) -> Result<Vec<String>, AppError> {
        let params = json!({
            "usergroup": user_group,
        });

        let response: UserGroupUsersResponse = self.send_request("usergroups.users.list", Method::GET, Some(&params), None).await?;

        Ok(response.users.unwrap_or_default())
    }

    pub async fn update_user_group_users(&self, user_group: &str, users: &Vec<String>) -> Result<(), AppError> {
        let payload = json!({
            "usergroup": user_group,
            "users": users,
        });

        self.send_request::<EmptyResponse, ()>("usergroups.users.update", Method::POST, None, Some(&payload)).await?;

        Ok(())
    }

    pub async fn update_channel_topic(&self, channel_id: &str, topic: &str) -> Result<Option<Channel>, AppError> {
        let payload = json!({
            "channel": channel_id,
            "topic": topic,
        });

        let response: ChannelResponse = self.send_request::<_, ()>("conversations.setTopic", Method::POST, None, Some(&payload)).await?;

        Ok(response.channel)
    }

    async fn send_request<T, Q>(&self, endpoint: &str, method: Method, params: Option<&Q>, payload: Option<&Value>) -> Result<T, AppError>
    where 
        T: for<'a> serde::Deserialize<'a>,
        Q: serde::Serialize,
    {
        let url = format!("https://slack.com/api/{}", endpoint);

        let mut request_builder = self.http_client.request(method.clone(), url)
            .bearer_auth(&self.api_token)
            .header("Content-Type", "application/json");

        if let Some(params) = params {
            request_builder = request_builder.query(params);
        }

        if let Some(payload) = payload {
            let body: String = payload.to_string();
            println!("Slack: {} {}: {}", method.as_str(), endpoint, &body);
            request_builder = request_builder.body(body);
        }

        let response = request_builder
            .send()
            .await?;

        if response.status().is_success() {
            let json_response: SlackResponse<T> = response.json().await?;

            if json_response.ok {
                // println!("Slack request successfully");
                Ok(json_response.data)
            } else if let Some(error) = json_response.error {
                println!("SlackClient: Failed to call Slack API, error message: {}", error);
                Err(AppError::SlackError(error))
            } else {
                println!("SlackClient: Unknown error occurred");
                Err(AppError::SlackError("Unknown error".to_string()))
            }
        } else {
            println!("SlackClient: Failed sending request to Slack, status: {}, Error: {:?}", response.status(), response);
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

pub async fn swap_slack_access_token(http_client: &Client, temp_token: &str, slack_client_id: &str, slack_client_secret: &str) -> Result<SlackOauthResponse, AppError> {
    println!("Swap slack access token");
    let params = json!({
        "code": temp_token,
    });

    let response = http_client
        .request(Method::POST, "https://slack.com/api/oauth.v2.access")
        .header("Authorization", format!("Basic {}", encode_with_pad(format!("{}:{}", slack_client_id, slack_client_secret).as_bytes())))
        .query(&params)
        .send()
        .await?;

    if response.status().is_success() {
        let response_body = response.text().await?;

        let json_response_result: Result<SlackResponse<SlackOauthResponse>, Error> = serde_json::from_str(&response_body);

        match json_response_result {
            Err(err) => {
                println!("Failed to parse json response: {}", response_body);
                Err(AppError::SlackError(err.to_string()))
            },
            Ok(json_response) => {
                if json_response.ok {
                    // println!("Slack request successfully");
                    Ok(json_response.data)
                } else if let Some(error) = json_response.error {
                    println!("SlackClient: Failed to call Slack API, error message: {}", error);
                    Err(AppError::SlackError(error))
                } else {
                    println!("SlackClient: Unknown error occurred");
                    Err(AppError::SlackError("Unknown error".to_string()))
                }
            }
        }
    } else {
        println!("SlackClient: Failed sending request to Slack, status: {}, Error: {:?}", response.status(), response);
        Err(AppError::SlackError(format!("Failed sending request to Slack, status: {}", response.status())))
    }        
}