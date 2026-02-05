use crate::errors::AppError;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ExternalSelectRequest {
    pub user: ExternalSelectUser,
    pub team: ExternalSelectTeam,
    #[serde(default)]
    pub enterprise: Option<ExternalSelectEnterprise>,
    pub action_id: String,
    pub block_id: String,
    pub value: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExternalSelectUser {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct ExternalSelectTeam {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct ExternalSelectEnterprise {
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct SlackInteractiveActionRequest {
    pub payload: String,
}

pub fn parse_slack_request(
    request_body: &str,
) -> Result<ExternalSelectRequest, AppError> {
    let params: SlackInteractiveActionRequest = serde_urlencoded::from_str(request_body)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse request body: {}", e)))?;

    let request: ExternalSelectRequest = serde_json::from_str(&params.payload)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse payload JSON: {}", e)))?;

    Ok(request)
}
