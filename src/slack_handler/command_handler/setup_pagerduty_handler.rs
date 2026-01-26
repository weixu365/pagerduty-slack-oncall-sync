use crate::{
    db::SlackInstallationRepository,
    errors::AppError,
    service_provider::pager_duty::PagerDuty,
    slack_handler::command_handler::slack_request::{SetupPagerdutyArgs, SlackCommandRequest},
    utils::http_client::build_http_client,
};

pub async fn handle_setup_pagerduty_command(
    params: SlackCommandRequest,
    arg: SetupPagerdutyArgs,
    slack_installations_db: &dyn SlackInstallationRepository,
) -> Result<Vec<String>, AppError> {
    let http_client = std::sync::Arc::new(build_http_client()?);
    let pager_duty = PagerDuty::new(http_client.clone(), arg.pagerduty_api_key.clone(), "".into());
    pager_duty.validate_token().await?;

    slack_installations_db
        .update_pagerduty_token(params.team_id, params.enterprise_id, &arg.pagerduty_api_key)
        .await?;

    Ok(vec![format!("PagerDuty API key validated and saved successfully")])
}
