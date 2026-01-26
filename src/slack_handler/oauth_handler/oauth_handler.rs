use aws_lambda_events::{apigw::ApiGatewayProxyResponse, query_map::QueryMap};

use crate::{
    aws::secrets_client::Secrets,
    db::{SlackInstallation, SlackInstallationRepository, dynamodb::SlackInstallationsDynamoDb},
    errors::AppError,
    service_provider::slack::swap_slack_access_token,
    slack_handler::utils::slack_response::response,
    utils::http_client::build_http_client,
};

pub async fn handle_slack_oauth(
    slack_installations_db: SlackInstallationsDynamoDb,
    secrets: &Secrets,
    query_map: QueryMap,
) -> Result<ApiGatewayProxyResponse, AppError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_query_map_with_code(code: &str) -> QueryMap {
        let mut map = HashMap::new();
        map.insert("code".to_string(), vec![code.to_string()]);
        QueryMap::from(map)
    }

    fn create_empty_query_map() -> QueryMap {
        let map: HashMap<String, Vec<String>> = HashMap::new();
        QueryMap::from(map)
    }

    #[test]
    fn test_query_map_with_code() -> Result<(), AppError> {
        let query_map = create_query_map_with_code("test_auth_code_123");

        let code = query_map.first("code");
        assert!(code.is_some());
        assert_eq!(code.unwrap(), "test_auth_code_123");

        Ok(())
    }

    #[test]
    fn test_query_map_without_code() -> Result<(), AppError> {
        let query_map = create_empty_query_map();

        let code = query_map.first("code");
        assert!(code.is_none());

        Ok(())
    }

    #[test]
    fn test_query_map_with_multiple_params() -> Result<(), AppError> {
        let mut map = HashMap::new();
        map.insert("code".to_string(), vec!["auth_code".to_string()]);
        map.insert("state".to_string(), vec!["some_state".to_string()]);
        let query_map = QueryMap::from(map);

        let code = query_map.first("code");
        assert!(code.is_some());
        assert_eq!(code.unwrap(), "auth_code");

        let state = query_map.first("state");
        assert!(state.is_some());
        assert_eq!(state.unwrap(), "some_state");

        Ok(())
    }

    #[test]
    fn test_query_map_empty() -> Result<(), AppError> {
        let query_map = create_empty_query_map();

        assert!(query_map.first("code").is_none());
        assert!(query_map.first("state").is_none());
        assert!(query_map.first("any_key").is_none());

        Ok(())
    }

    // Note: Full integration tests for handle_slack_oauth would require:
    // 1. Mocking the HTTP client for Slack OAuth API
    // 2. Mocking the swap_slack_access_token function
    // 3. Mocking the SlackInstallationsDynamoDb
    // These are better suited for integration tests with a proper mocking framework
}
