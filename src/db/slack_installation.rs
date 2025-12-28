
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
