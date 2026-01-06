use aws_lambda_events::{
    encodings::Body,
    query_map::QueryMap,
};
use lambda_http::Response;

use crate::{
    aws::secrets_client::Secrets,
    db::{
        dynamodb::SlackInstallationsDynamoDb,
        SlackInstallation, SlackInstallationRepository,
    },
    errors::AppError,
    service_provider::slack::swap_slack_access_token,
    utils::http_client::build_http_client,
    utils::http_util::response,
};

pub async fn handle_slack_oauth(
    slack_installations_db: SlackInstallationsDynamoDb,
    secrets: &Secrets,
    query_map: QueryMap,
) -> Result<Response<Body>, AppError> {
    let code_parameter = query_map.first("code");

    match code_parameter {
        Some(temporary_code) => {
            let http_client = build_http_client()?;
            let oauth_response = swap_slack_access_token(
                &http_client,
                temporary_code,
                &secrets.slack_client_id,
                &secrets.slack_client_secret,
            )
            .await?;

            let installation = SlackInstallation {
                team_id: oauth_response.team.id,
                team_name: oauth_response.team.name,
                enterprise_id: oauth_response.enterprise.id,
                enterprise_name: oauth_response.enterprise.name,
                is_enterprise_install: oauth_response.is_enterprise_install,

                access_token: oauth_response.access_token,
                token_type: oauth_response.token_type,
                scope: oauth_response.scope,

                authed_user_id: oauth_response.authed_user.id,
                app_id: oauth_response.app_id,
                bot_user_id: oauth_response.bot_user_id,

                pager_duty_token: None,
            };

            slack_installations_db.save_slack_installation(&installation).await?;
            response(200, format!("Received slack oauth callback."))
        }
        None => response(400, format!("Invalid request")),
    }
}
