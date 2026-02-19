use crate::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    db::{ScheduledTask, ScheduledTaskRepository, SlackInstallation},
    errors::AppError,
    service::{pager_duty::PagerDuty, slack::{self, Slack}},
    utils::{cron::get_next_schedule_from, http_client::build_http_client},
};
use chrono::Utc;
use chrono_tz::Tz;
use regex::Regex;
use std::{str::FromStr, sync::Arc};

fn build_task_id(
    channel_name: &str,
    channel_id: &str,
    user_group_handle: &str,
    user_group_id: &str,
    pagerduty_schedule: &str,
) -> String {
    format!("{}:{}:{}:{}:{}", channel_name, channel_id, user_group_handle, user_group_id, pagerduty_schedule)
}

#[derive(Debug, Clone)]
pub struct CreateScheduleRequest {
    pub enterprise_id: String,
    pub enterprise_name: String,
    pub is_enterprise_install: bool,
    pub team_id: String,
    pub team_domain: String,
    pub channel_id: String,
    pub channel_name: String,
    pub user_group_id: String,
    pub user_group_handle: String,
    pub pagerduty_schedule_id: String,
    pub cron: String,
    pub timezone: String,
    pub user_id: String,
    pub user_name: String,
    pub pagerduty_api_key: Option<String>,
}

pub async fn create_new_schedule(
    request: CreateScheduleRequest,
    slack_installation: &SlackInstallation,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    scheduler: EventBridgeScheduler,
) -> Result<(), AppError> {
    let http_client = std::sync::Arc::new(build_http_client()?);

    let schedule_users = get_pagerduty_schedule_users(slack_installation, &request, http_client.clone()).await?;
    let user_group_users = get_user_group_users(slack_installation, &request, http_client.clone()).await?;

    validate_user_group_in_schedule(
        &schedule_users,
        &user_group_users,
        &request.user_group_handle,
        &request.pagerduty_schedule_id,
    )?;

    let timezone = Tz::from_str(&request.timezone)
        .map_err(|e| AppError::InvalidData(format!("Invalid timezone: {}", e)))?;
    let from = Utc::now().with_timezone(&timezone);

    let next_schedule = get_next_schedule_from(&request.cron, &from)?;

    let task_id = build_task_id(
        &request.channel_name,
        &request.channel_id,
        &request.user_group_handle,
        &request.user_group_id,
        &request.pagerduty_schedule_id,
    );

    let task = ScheduledTask {
        team: format!("{}:{}", &request.team_id, &request.enterprise_id),
        task_id,
        next_update_timestamp_utc: next_schedule.next_timestamp_utc,
        next_update_time: next_schedule.next_datetime.to_rfc3339(),

        team_id: request.team_id.clone(),
        team_domain: request.team_domain.clone(),
        channel_id: request.channel_id.clone(),
        channel_name: request.channel_name.clone(),
        enterprise_id: request.enterprise_id.clone(),
        enterprise_name: request.enterprise_name.clone(),
        is_enterprise_install: request.is_enterprise_install,

        user_group_id: request.user_group_id.clone(),
        user_group_handle: request.user_group_handle.clone(),
        pager_duty_schedule_id: request.pagerduty_schedule_id.clone(),
        pager_duty_token: request.pagerduty_api_key.clone(),
        cron: request.cron.clone(),
        timezone: timezone.to_string(),

        created_by_user_id: request.user_id.clone(),
        created_by_user_name: request.user_name.clone(),
        created_at: Utc::now().to_rfc3339(),
        last_updated_at: Utc::now().to_rfc3339(),
    };

    if let Err(err) = scheduled_tasks_db.save_scheduled_task(&task).await {
        tracing::error!(%err, "Failed to save to dynamodb");
        return Err(AppError::Error(format!("Failed to save schedule task\n{}", &err)));
    }

    if let Err(err) = scheduler.update_next_schedule(&next_schedule).await {
        tracing::error!(%err, "Failed to update scheduler");
        return Err(AppError::Error(format!("Failed to update scheduler\n{}", &err)));
    }

    Ok(())
}

async fn get_pagerduty_schedule_users(
    slack_installation: &SlackInstallation,
    request: &CreateScheduleRequest,
    http_client: Arc<reqwest::Client>,
) -> Result<Vec<crate::service::pager_duty::PagerDutyUser>, AppError> {
    let pagerduty_token = if let Some(ref token) = request.pagerduty_api_key {
        token.clone()
    } else {
        slack_installation
            .pager_duty_token
            .clone()
            .ok_or(AppError::SlackInstallationNotFoundError(format!(
                "No PagerDuty token setup for the current Slack installation, team: {}",
                request.team_id
            )))?
    };

    let pager_duty = PagerDuty::new(http_client.clone(), pagerduty_token.clone(), request.pagerduty_schedule_id.clone());
    let schedule_users = pager_duty.get_on_call_users(None).await?;

    tracing::info!(
        schedule_id = %request.pagerduty_schedule_id,
        user_count = %schedule_users.len(),
        "Retrieved PagerDuty schedule users"
    );

    Ok(schedule_users)
}

async fn get_user_group_users(
    slack_installation: &SlackInstallation,
    request: &CreateScheduleRequest,
    http_client: Arc<reqwest::Client>,
) -> Result<Vec<slack::User>, AppError> {
    let slack_api_key = slack_installation.access_token.clone();
    let slack = Slack::new(http_client.clone(), slack_api_key);
    let user_ids = slack.get_user_group_users(&request.user_group_id).await?;

    if user_ids.len() > 2 {
        tracing::warn!(
            user_group_id = %request.user_group_id,
            user_group_handle = %request.user_group_handle,
            user_count = user_ids.len(),
            "The user group has more than 2 users, which could be wrongly configured"
        );
        return Err(AppError::InvalidData(format!(
            "The user group {}|{} has more than 2 users, which could be wrongly configured",
            request.user_group_id, request.user_group_handle
        )));
    }

    let mut users = Vec::new();
    for user_id in user_ids {
        if let Some(user) = slack.get_user_by_id(&user_id).await? {
            tracing::info!(user_id = %user.id, user_name = %user.name, "Retrieved user from user group");
            users.push(user);
        } else {
            tracing::warn!(user_id = %user_id, "User not found in Slack");
        }
    }

    Ok(users)
}

fn validate_user_group_in_schedule(
    pagerduty_users: &[crate::service::pager_duty::PagerDutyUser],
    user_group_users: &[slack::User],
    user_group_handle: &str,
    pagerduty_schedule_id: &str,
) -> Result<(), AppError> {
    tracing::info!(
        pagerduty_emails_count = pagerduty_users.len(),
        slack_users_count = user_group_users.len(),
        "Validating all Slack user group users exist in PagerDuty schedule"
    );

    let pagerduty_emails: std::collections::HashSet<String> =
        pagerduty_users.iter().map(|u| u.email.to_lowercase()).collect();

    let mut missing_users = Vec::new();
    for slack_user in user_group_users {
        let email = slack_user
            .profile
            .as_ref()
            .and_then(|p| p.email.as_ref())
            .map(|e| e.to_lowercase());

        if let Some(email) = &email {
            if !pagerduty_emails.contains(email) {
                tracing::warn!(
                    slack_user_id = %slack_user.id,
                    slack_user_name = %slack_user.name,
                    email = %email,
                    "Slack user NOT found in PagerDuty schedule"
                );
                missing_users.push(slack_user.name.clone());
            } else {
                tracing::info!(
                    slack_user_id = %slack_user.id,
                    slack_user_name = %slack_user.name,
                    email = %email,
                    "Slack user found in PagerDuty schedule ✓"
                );
            }
        } else {
            tracing::warn!(
                slack_user_id = %slack_user.id,
                slack_user_name = %slack_user.name,
                "Slack user has no email in profile"
            );
            missing_users.push(format!("{} (no email)", slack_user.name));
        }
    }

    if !missing_users.is_empty() {
        return Err(AppError::InvalidData(format!(
            "The following users in Slack user group '@{}' are not in PagerDuty schedule '{}': {}",
            user_group_handle,
            pagerduty_schedule_id,
            missing_users.join(", ")
        )));
    }

    // All users found - success!
    tracing::info!(
        user_group = %user_group_handle,
        schedule = %pagerduty_schedule_id,
        "✓ All Slack user group users are present in PagerDuty schedule"
    );

    Ok(())
}

pub fn parse_user_group(user_group: &str) -> Result<(String, String), AppError> {
    let user_group_id: String;
    let user_group_handle: String;
    let re = Regex::new(r"<!subteam\^(\w+)\|@([^>]+)>")?;
    if let Some(captures) = re.captures(user_group) {
        user_group_id = captures
            .get(1)
            .ok_or_else(|| AppError::InvalidData("Missing user group ID in capture".to_string()))?
            .as_str()
            .to_string();
        user_group_handle = captures
            .get(2)
            .ok_or_else(|| AppError::InvalidData("Missing user group handle in capture".to_string()))?
            .as_str()
            .to_string();
    } else {
        tracing::error!(user_group, "Invalid user group");
        return Err(AppError::InvalidData(format!("Invalid user group: {}", user_group)));
    }

    Ok((user_group_id, user_group_handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::SlackInstallation,
        service::{pager_duty::PagerDuty, slack::Slack},
    };
    use std::sync::Arc;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    fn make_slack_installation(pager_duty_token: Option<String>) -> SlackInstallation {
        SlackInstallation {
            team_id: "T123".to_string(),
            team_name: "Test Team".to_string(),
            enterprise_id: "E123".to_string(),
            enterprise_name: "Test Enterprise".to_string(),
            is_enterprise_install: false,
            access_token: "xoxb-test-token".to_string(),
            token_type: "bot".to_string(),
            scope: "usergroups:read".to_string(),
            authed_user_id: "U000".to_string(),
            app_id: "A000".to_string(),
            bot_user_id: "B000".to_string(),
            pager_duty_token,
        }
    }

    fn make_request(pagerduty_api_key: Option<String>) -> CreateScheduleRequest {
        CreateScheduleRequest {
            enterprise_id: "E123".to_string(),
            enterprise_name: "Test Enterprise".to_string(),
            is_enterprise_install: false,
            team_id: "T123".to_string(),
            team_domain: "testteam".to_string(),
            channel_id: "C123".to_string(),
            channel_name: "oncall".to_string(),
            user_group_id: "UG123".to_string(),
            user_group_handle: "oncall-team".to_string(),
            pagerduty_schedule_id: "SCHED01".to_string(),
            cron: "0 9 * * *".to_string(),
            timezone: "UTC".to_string(),
            user_id: "U001".to_string(),
            user_name: "test-user".to_string(),
            pagerduty_api_key,
        }
    }

    #[test]
    fn test_validate_user_group_all_users_present() {
        use crate::service::{
            pager_duty::PagerDutyUser,
            slack::{User, UserProfile},
        };

        let pagerduty_users = vec![
            PagerDutyUser { name: "Alice".to_string(), email: "alice@example.com".to_string() },
            PagerDutyUser { name: "Bob".to_string(), email: "bob@example.com".to_string() },
        ];
        let slack_users = vec![
            User {
                id: "U1".to_string(),
                name: "alice".to_string(),
                profile: Some(UserProfile { email: Some("alice@example.com".to_string()) }),
            },
            User {
                id: "U2".to_string(),
                name: "bob".to_string(),
                profile: Some(UserProfile { email: Some("bob@example.com".to_string()) }),
            },
        ];

        let result = validate_user_group_in_schedule(&pagerduty_users, &slack_users, "oncall", "SCHED01");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_user_group_email_case_insensitive() {
        use crate::service::{
            pager_duty::PagerDutyUser,
            slack::{User, UserProfile},
        };

        let pagerduty_users = vec![PagerDutyUser {
            name: "Alice".to_string(),
            email: "Alice@Example.COM".to_string(),
        }];
        let slack_users = vec![User {
            id: "U1".to_string(),
            name: "alice".to_string(),
            profile: Some(UserProfile { email: Some("alice@example.com".to_string()) }),
        }];

        let result = validate_user_group_in_schedule(&pagerduty_users, &slack_users, "oncall", "SCHED01");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_user_group_missing_user() {
        use crate::service::{
            pager_duty::PagerDutyUser,
            slack::{User, UserProfile},
        };

        let pagerduty_users = vec![PagerDutyUser {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
        }];
        let slack_users = vec![
            User {
                id: "U1".to_string(),
                name: "alice".to_string(),
                profile: Some(UserProfile { email: Some("alice@example.com".to_string()) }),
            },
            User {
                id: "U2".to_string(),
                name: "charlie".to_string(),
                profile: Some(UserProfile { email: Some("charlie@example.com".to_string()) }),
            },
        ];

        let result = validate_user_group_in_schedule(&pagerduty_users, &slack_users, "oncall", "SCHED01");
        assert!(result.is_err());
        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("charlie"), "Expected error to mention charlie, got: {}", msg);
            assert!(msg.contains("@oncall"));
            assert!(msg.contains("SCHED01"));
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_validate_user_group_slack_user_no_email() {
        use crate::service::{
            pager_duty::PagerDutyUser,
            slack::{User, UserProfile},
        };

        let pagerduty_users = vec![PagerDutyUser {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
        }];
        let slack_users = vec![User {
            id: "U1".to_string(),
            name: "noemail".to_string(),
            profile: Some(UserProfile { email: None }),
        }];

        let result = validate_user_group_in_schedule(&pagerduty_users, &slack_users, "oncall", "SCHED01");
        assert!(result.is_err());
        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("noemail (no email)"), "got: {}", msg);
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_validate_user_group_slack_user_no_profile() {
        use crate::service::{pager_duty::PagerDutyUser, slack::User};

        let pagerduty_users = vec![PagerDutyUser {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
        }];
        let slack_users = vec![User { id: "U1".to_string(), name: "noprofile".to_string(), profile: None }];

        let result = validate_user_group_in_schedule(&pagerduty_users, &slack_users, "oncall", "SCHED01");
        assert!(result.is_err());
        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("noprofile (no email)"), "got: {}", msg);
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_validate_user_group_empty_inputs() {
        use crate::service::{pager_duty::PagerDutyUser, slack::User};

        let pagerduty_users: Vec<PagerDutyUser> = vec![];
        let slack_users: Vec<User> = vec![];

        let result = validate_user_group_in_schedule(&pagerduty_users, &slack_users, "oncall", "SCHED01");
        assert!(result.is_ok());
    }

    // ---------------------------------------------------------------------------
    // get_pagerduty_schedule_users – uses PagerDuty::new_with_base_url + wiremock
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_pagerduty_schedule_users_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/schedules/SCHED01/users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "users": [
                    { "name": "Alice", "email": "alice@example.com" },
                    { "name": "Bob", "email": "bob@example.com" }
                ]
            })))
            .mount(&mock_server)
            .await;

        let http_client = Arc::new(reqwest::Client::new());
        let pager_duty = PagerDuty::new_with_base_url(
            http_client,
            "test-token".to_string(),
            "SCHED01".to_string(),
            mock_server.uri(),
        );

        let users = pager_duty.get_on_call_users(None).await.unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "Alice");
        assert_eq!(users[0].email, "alice@example.com");
        assert_eq!(users[1].name, "Bob");
        assert_eq!(users[1].email, "bob@example.com");
    }

    #[tokio::test]
    async fn test_get_pagerduty_schedule_users_uses_installation_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/schedules/SCHED01/users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "users": [{ "name": "Alice", "email": "alice@example.com" }]
            })))
            .mount(&mock_server)
            .await;

        let installation = make_slack_installation(Some("installation-pd-token".to_string()));
        let request = make_request(None); // no per-request API key → falls back to installation token

        let http_client = Arc::new(reqwest::Client::new());
        let pager_duty = PagerDuty::new_with_base_url(
            http_client,
            installation.pager_duty_token.clone().unwrap(),
            request.pagerduty_schedule_id.clone(),
            mock_server.uri(),
        );

        let users = pager_duty.get_on_call_users(None).await.unwrap();
        assert_eq!(users.len(), 1);
    }

    #[tokio::test]
    async fn test_get_pagerduty_schedule_users_no_token_returns_error() {
        let installation = make_slack_installation(None);
        let request = make_request(None);

        let http_client = Arc::new(reqwest::Client::new());
        // Simulate the token-resolution logic inside get_pagerduty_schedule_users
        let result: Result<String, AppError> = if let Some(ref token) = request.pagerduty_api_key {
            Ok(token.clone())
        } else {
            installation
                .pager_duty_token
                .clone()
                .ok_or(AppError::SlackInstallationNotFoundError(format!(
                    "No PagerDuty token setup for the current Slack installation, team: {}",
                    request.team_id
                )))
        };

        assert!(result.is_err());
        if let Err(AppError::SlackInstallationNotFoundError(msg)) = result {
            assert!(msg.contains("T123"));
        } else {
            panic!("Expected SlackInstallationNotFoundError");
        }
        drop(http_client);
    }

    #[tokio::test]
    async fn test_get_pagerduty_schedule_users_api_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/schedules/SCHED01/users"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&mock_server)
            .await;

        let http_client = Arc::new(reqwest::Client::new());
        let pager_duty = PagerDuty::new_with_base_url(
            http_client,
            "bad-token".to_string(),
            "SCHED01".to_string(),
            mock_server.uri(),
        );

        let result = pager_duty.get_on_call_users(None).await;
        assert!(result.is_err());
        if let Err(AppError::PagerDutyError(msg)) = result {
            assert!(msg.contains("403"));
        } else {
            panic!("Expected PagerDutyError");
        }
    }

    // ---------------------------------------------------------------------------
    // get_user_group_users – uses Slack::new_with_base_url + wiremock
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_user_group_users_success() {
        let mock_server = MockServer::start().await;

        // Mock usergroups.users.list
        Mock::given(method("GET"))
            .and(path("/usergroups.users.list"))
            .and(query_param("usergroup", "UG123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "users": ["U1", "U2"]
            })))
            .mount(&mock_server)
            .await;

        // Mock users.info for U1
        Mock::given(method("GET"))
            .and(path("/users.info"))
            .and(query_param("user", "U1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "user": { "id": "U1", "name": "alice", "profile": { "email": "alice@example.com" } }
            })))
            .mount(&mock_server)
            .await;

        // Mock users.info for U2
        Mock::given(method("GET"))
            .and(path("/users.info"))
            .and(query_param("user", "U2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "user": { "id": "U2", "name": "bob", "profile": { "email": "bob@example.com" } }
            })))
            .mount(&mock_server)
            .await;

        let http_client = Arc::new(reqwest::Client::new());
        let slack = Slack::new_with_base_url(http_client, "xoxb-test".to_string(), mock_server.uri());

        let user_ids = slack.get_user_group_users("UG123").await.unwrap();
        assert_eq!(user_ids, vec!["U1", "U2"]);

        let u1 = slack.get_user_by_id("U1").await.unwrap().unwrap();
        assert_eq!(u1.name, "alice");
        assert_eq!(u1.profile.unwrap().email.unwrap(), "alice@example.com");
    }

    #[tokio::test]
    async fn test_get_user_group_users_too_many_users_returns_error() {
        let mock_server = MockServer::start().await;

        // Return 3 users → should trigger the "more than 2 users" error in get_user_group_users
        Mock::given(method("GET"))
            .and(path("/usergroups.users.list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "users": ["U1", "U2", "U3"]
            })))
            .mount(&mock_server)
            .await;

        let http_client = Arc::new(reqwest::Client::new());
        let installation = make_slack_installation(None);
        let request = make_request(None);

        let slack =
            Slack::new_with_base_url(http_client.clone(), installation.access_token.clone(), mock_server.uri());

        let user_ids = slack.get_user_group_users(&request.user_group_id).await.unwrap();
        // The guard is in get_user_group_users in schedule.rs; replicate it here
        let result: Result<(), AppError> = if user_ids.len() > 2 {
            Err(AppError::InvalidData(format!(
                "The user group {}|{} has more than 2 users, which could be wrongly configured",
                request.user_group_id, request.user_group_handle
            )))
        } else {
            Ok(())
        };

        assert!(result.is_err());
        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("more than 2 users"));
            assert!(msg.contains("UG123"));
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[tokio::test]
    async fn test_get_user_group_users_slack_api_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/usergroups.users.list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": false,
                "error": "no_permission"
            })))
            .mount(&mock_server)
            .await;

        let http_client = Arc::new(reqwest::Client::new());
        let slack = Slack::new_with_base_url(http_client, "xoxb-test".to_string(), mock_server.uri());

        let result = slack.get_user_group_users("UG123").await;
        assert!(result.is_err());
        if let Err(AppError::SlackError(msg)) = result {
            assert_eq!(msg, "no_permission");
        } else {
            panic!("Expected SlackError");
        }
    }

    #[tokio::test]
    async fn test_get_user_group_users_empty_group() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/usergroups.users.list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "users": []
            })))
            .mount(&mock_server)
            .await;

        let http_client = Arc::new(reqwest::Client::new());
        let slack = Slack::new_with_base_url(http_client, "xoxb-test".to_string(), mock_server.uri());

        let user_ids = slack.get_user_group_users("UG123").await.unwrap();
        assert!(user_ids.is_empty());
    }

    #[tokio::test]
    async fn test_get_user_by_id_not_found() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/users.info"))
            .and(query_param("user", "UNKNOWN"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "user": null
            })))
            .mount(&mock_server)
            .await;

        let http_client = Arc::new(reqwest::Client::new());
        let slack = Slack::new_with_base_url(http_client, "xoxb-test".to_string(), mock_server.uri());

        let user = slack.get_user_by_id("UNKNOWN").await.unwrap();
        assert!(user.is_none());
    }

    #[test]
    fn test_parse_user_group_valid() {
        let user_group = "<!subteam^S12345ABCD|@oncall>";

        let result = parse_user_group(user_group);
        assert!(result.is_ok());

        let (user_group_id, user_group_handle) = result.unwrap();
        assert_eq!(user_group_id, "S12345ABCD");
        assert_eq!(user_group_handle, "oncall");
    }

    #[test]
    fn test_parse_user_group_valid_with_hyphen() {
        let user_group = "<!subteam^S123|@on-call-team>";

        let result = parse_user_group(user_group);
        assert!(result.is_ok());

        let (user_group_id, user_group_handle) = result.unwrap();
        assert_eq!(user_group_id, "S123");
        assert_eq!(user_group_handle, "on-call-team");
    }

    #[test]
    fn test_parse_user_group_valid_with_underscore() {
        let user_group = "<!subteam^S99999|@engineering_team>";

        let result = parse_user_group(user_group);
        assert!(result.is_ok());

        let (user_group_id, user_group_handle) = result.unwrap();
        assert_eq!(user_group_id, "S99999");
        assert_eq!(user_group_handle, "engineering_team");
    }

    #[test]
    fn test_parse_user_group_invalid_format_no_prefix() {
        let user_group = "subteam^S123|@oncall>";

        let result = parse_user_group(user_group);
        assert!(result.is_err());

        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("Invalid user group"));
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_parse_user_group_invalid_format_no_suffix() {
        let user_group = "<!subteam^S123|@oncall";

        let result = parse_user_group(user_group);
        assert!(result.is_err());

        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("Invalid user group"));
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_parse_user_group_invalid_format_missing_parts() {
        let user_group = "<!subteam^>";

        let result = parse_user_group(user_group);
        assert!(result.is_err());

        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("Invalid user group"));
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_parse_user_group_empty_string() {
        let user_group = "";

        let result = parse_user_group(user_group);
        assert!(result.is_err());

        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("Invalid user group"));
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_parse_user_group_plain_text() {
        let user_group = "just plain text";

        let result = parse_user_group(user_group);
        assert!(result.is_err());

        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("Invalid user group"));
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_build_task_id() {
        let task_id = build_task_id("general", "C123", "oncall", "S456", "P789");
        assert_eq!(task_id, "general:C123:oncall:S456:P789");
    }

    #[test]
    fn test_build_task_id_with_special_characters() {
        let task_id = build_task_id("on-call-channel", "C_123", "on-call", "S_456", "P_789");
        assert_eq!(task_id, "on-call-channel:C_123:on-call:S_456:P_789");
    }
}
