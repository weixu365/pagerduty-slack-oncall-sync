use crate::errors::AppError;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct SlackInstallation {
    pub team_id: String,
    pub team_name: String,
    pub enterprise_id: String,
    pub enterprise_name: String,
    pub is_enterprise_install: bool,

    pub access_token: String,
    pub token_type: String,
    pub scope: String,

    pub authed_user_id: String,
    pub app_id: String,
    pub bot_user_id: String,

    pub pager_duty_token: Option<String>,
}

#[async_trait]
pub trait SlackInstallationRepository: Send + Sync {
    async fn save_slack_installation(&self, installation: &SlackInstallation) -> Result<(), AppError>;

    async fn update_pagerduty_token(
        &self,
        slack_team_id: String,
        slack_enterprise_id: String,
        pagerduty_token: &str,
    ) -> Result<(), AppError>;

    async fn get_slack_installation(
        &self,
        slack_team_id: &str,
        slack_enterprise_id: &str,
    ) -> Result<SlackInstallation, AppError>;

    async fn list_installations(&self) -> Result<Vec<SlackInstallation>, AppError>;
}
