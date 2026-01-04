use aws_lambda_events::{
    encodings::Body,
    http::{HeaderMap, HeaderValue},
    query_map::QueryMap,
};
use lambda_http::Response;
use std::env;

use crate::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    config::Config,
    db::{
        dynamodb::{ScheduledTasksDynamodb, SlackInstallationsDynamoDb},
        ScheduledTask, ScheduledTaskRepository, SlackInstallation, SlackInstallationRepository,
    },
    encryptor::Encryptor,
    errors::AppError,
    service_provider::{pager_duty::PagerDuty, slack::swap_slack_access_token},
    utils::constant_time::constant_time_compare_str,
    utils::cron::get_next_schedule_from,
    utils::http_client::build_http_client,
    utils::http_util::response,
};
use chrono::Utc;
use chrono_tz::Tz;
use clap::Parser;
use clap::{Args, Subcommand};
use lazy_static::lazy_static;
use regex::Regex;
use ring::hmac;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct App {
    #[clap(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Args)]
struct ScheduleArgs {
    #[arg(long)]
    user_group: String,

    #[arg(long, default_value = "0 9 ? * MON-FRI *")]
    pagerduty_schedule: String,

    #[arg(long)]
    pagerduty_api_key: Option<String>,

    #[arg(long)]
    cron: String,

    #[arg(long)]
    timezone: Option<String>,
}

#[derive(Debug, Args)]
struct SetupPagerdutyArgs {
    #[arg(long)]
    pagerduty_api_key: String,
}

#[derive(Debug, Args)]
struct ListSchedulesArgs {
    #[arg(long)]
    all: Option<bool>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Schedule(ScheduleArgs),
    ListSchedules(ListSchedulesArgs),
    SetupPagerduty(SetupPagerdutyArgs),
    New,
}

#[derive(Debug, Deserialize)]
struct SlackCommandRequest {
    team_id: String,
    team_domain: String,
    channel_id: String,
    channel_name: String,
    #[serde(default)]
    enterprise_id: String,
    #[serde(default)]
    enterprise_name: String,
    #[serde(default)]
    is_enterprise_install: String,
    user_id: String,
    user_name: String,
    command: String,
    #[serde(default)]
    text: String,
    // #[serde(default)]
    // response_url: String,
}

fn parse_slack_command_request(request_body: &str) -> Result<SlackCommandRequest, AppError> {
    serde_urlencoded::from_str(request_body)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse request body: {}", e)))
}

fn cleanse(text: &str) -> String {
    lazy_static! {
        static ref DOUBLE_QUOTES: Regex = Regex::new(r"[\u{201C}\u{201D}]").unwrap();
        static ref SINGLE_QUOTES: Regex = Regex::new(r"[\u{2018}\u{2019}]").unwrap();
    }

    let cleansed_double_quote = DOUBLE_QUOTES.replace_all(text, "\"");
    let cleansed = SINGLE_QUOTES.replace_all(&cleansed_double_quote, "'");

    cleansed.to_string()
}

fn build_task_id(
    channel_name: &str,
    channel_id: &str,
    user_group_handle: &str,
    user_group_id: &str,
    pagerduty_schedule: &str,
) -> String {
    format!("{}:{}:{}:{}:{}", channel_name, channel_id, user_group_handle, user_group_id, pagerduty_schedule)
}

fn validate_request(request_header: &HeaderMap<HeaderValue>, request_body: &str, slack_signing_secret: &str) -> Result<(), AppError> {
    let slack_request_timestamp = request_header
        .get("X-Slack-Request-Timestamp")
        .ok_or_else(|| AppError::InvalidSlackRequest("Missing X-Slack-Request-Timestamp header".to_string()))?
        .to_str()
        .map_err(|_| AppError::InvalidSlackRequest("Invalid X-Slack-Request-Timestamp encoding".to_string()))?
        .parse::<i64>()
        .map_err(|_| AppError::InvalidSlackRequest("Invalid X-Slack-Request-Timestamp value".to_string()))?;

    let slack_request_signature = request_header
        .get("X-Slack-Signature")
        .ok_or_else(|| AppError::InvalidSlackRequest("Missing X-Slack-Signature header".to_string()))?
        .to_str()
        .map_err(|_| AppError::InvalidSlackRequest("Invalid X-Slack-Signature encoding".to_string()))?;

    let now = chrono::Utc::now().timestamp();
    if (now - slack_request_timestamp).abs() > 60 * 5 {
        return Err(AppError::InvalidSlackRequest(format!("Invalid slack command: wrong timestamp")));
    }

    let sig_basestring = format!("v0:{}:{}", slack_request_timestamp, request_body);
    tracing::debug!(sig_basestring, "Slack Request to sign");
    
    let verification_key = hmac::Key::new(hmac::HMAC_SHA256, slack_signing_secret.as_bytes());
    let signature = hex::encode(hmac::sign(&verification_key, sig_basestring.as_bytes()).as_ref());
    let expected_signature = format!("v0={}", signature);

    if !constant_time_compare_str(&expected_signature, slack_request_signature) {
        tracing::error!(slack_request_signature, "Signature verification failed");
        return Err(AppError::InvalidSlackRequest(format!("Invalid slack command signature")));
    }

    Ok(())
}

pub async fn handle_slack_oauth(config: &Config, query_map: QueryMap) -> Result<Response<Body>, AppError> {
    let code_parameter = query_map.first("code");

    match code_parameter {
        Some(temporary_code) => {
            let http_client = build_http_client()?;
            let secrets = config.secrets().await?;
            let encryptor = Encryptor::from_key(&secrets.encryption_key)?;
            let oauth_response = swap_slack_access_token(
                &http_client,
                temporary_code,
                &secrets.slack_client_id,
                &secrets.slack_client_secret,
            )
            .await?;

            // Save to dynamodb
            let db = SlackInstallationsDynamoDb::new(&config, encryptor);
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

            db.save_slack_installation(&installation).await?;
            response(200, format!("Received slack oauth callback."))
        }
        None => response(400, format!("Invalid request")),
    }
}

pub async fn handle_slack_command(
    config: &Config,
    request_header: &HeaderMap<HeaderValue>,
    request_body: &str,
) -> Result<Response<Body>, AppError> {
    let params = parse_slack_command_request(request_body)?;
    tracing::debug!(?params, "params in request body");

    let secrets = config.secrets().await?;
    validate_request(request_header, request_body, &secrets.slack_signing_secret)?;

    let is_enterprise_install = params.is_enterprise_install.eq_ignore_ascii_case("true");

    let arg = match shlex::split(cleanse(format!("{} {}", &params.command, &params.text).as_str()).as_str()) {
        Some(args) => Some(App::parse_from(args.iter())),
        None => None,
    };

    let encryptor = Encryptor::from_key(&secrets.encryption_key)?;

    let arg = arg.ok_or_else(|| AppError::InvalidData("Failed to parse command arguments".to_string()))?;

    let response_body = match arg.command {
        Some(Command::Schedule(arg)) => {
            let (user_group_id, user_group_handle) = parse_user_group(&arg.user_group)?;
            let http_client = std::sync::Arc::new(build_http_client()?);

            let pagerduty_token = if let Some(ref token) = arg.pagerduty_api_key {
                token.clone()
            } else {
                let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
                slack_installations_db
                    .get_slack_installation(&params.team_id, &params.enterprise_id)
                    .await?
                    .pager_duty_token
                    .ok_or(AppError::SlackInstallationNotFoundError(format!(
                        "No PagerDuty token setup for the current Slack installation, team: {}",
                        params.team_id
                    )))?
            };

            // Validate PagerDuty token and schedule by making a test API call
            let pager_duty =
                PagerDuty::new(http_client.clone(), pagerduty_token.clone(), arg.pagerduty_schedule.clone());
            pager_duty.validate_token().await?;
            pager_duty.get_on_call_users(Utc::now()).await?;

            let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
            let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;

            let db = ScheduledTasksDynamodb::new(&config, encryptor);
            let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);

            let timezone = Tz::from_str(&arg.timezone.unwrap_or("UTC".to_string()))
                .map_err(|e| AppError::InvalidData(format!("Invalid timezone: {}", e)))?;
            let from = Utc::now().with_timezone(&timezone);

            let next_schedule = get_next_schedule_from(&arg.cron, &from)?;

            let task_id = build_task_id(
                &params.channel_name,
                &params.channel_id,
                &user_group_handle,
                &user_group_id,
                &arg.pagerduty_schedule,
            );

            let task = ScheduledTask {
                team: format!("{}:{}", &params.team_id, &params.enterprise_id),
                task_id,
                next_update_timestamp_utc: next_schedule.next_timestamp_utc,
                next_update_time: next_schedule.next_datetime.to_rfc3339(),

                team_id: params.team_id,
                team_domain: params.team_domain,
                channel_id: params.channel_id,
                channel_name: params.channel_name,
                enterprise_id: params.enterprise_id,
                enterprise_name: params.enterprise_name,
                is_enterprise_install,

                user_group_id,
                user_group_handle,
                pager_duty_schedule_id: arg.pagerduty_schedule,
                pager_duty_token: arg.pagerduty_api_key,
                cron: arg.cron,
                timezone: timezone.to_string(),

                created_by_user_id: params.user_id,
                created_by_user_name: params.user_name,
                created_at: Utc::now().to_rfc3339(),
                last_updated_at: Utc::now().to_rfc3339(),
            };

            if let Err(err) = db.save_scheduled_task(&task).await {
                tracing::error!(%err, "Failed to save to dynamodb");
                return response(500, format!("Failed to save schedule task\n{} {}", &params.command, &params.text));
            }

            if let Err(err) = scheduler.update_next_schedule(&next_schedule).await {
                tracing::error!(%err, "Failed to update scheduler");
                return response(500, format!("Failed to update scheduler\n{} {}", &params.command, &params.text));
            }

            vec![format!(
                "Update user group: {}|{} based on pagerduty schedule: {}, at: {}",
                task.user_group_id, task.user_group_handle, &task.pager_duty_schedule_id, &task.cron
            )]
        }
        Some(Command::SetupPagerduty(args)) => {
            let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());

            let http_client = std::sync::Arc::new(build_http_client()?);
            let pager_duty = PagerDuty::new(http_client.clone(), args.pagerduty_api_key.clone(), "".into());
            pager_duty.validate_token().await?;

            slack_installations_db
                .update_pagerduty_token(params.team_id, params.enterprise_id, &args.pagerduty_api_key)
                .await?;

            vec![format!("PagerDuty API key validated and saved successfully")]
        }
        Some(Command::ListSchedules(_args)) => {
            let db = ScheduledTasksDynamodb::new(&config, encryptor);
            let tasks = db.list_scheduled_tasks().await?;

            tasks
                .into_iter()
                .map(|t| {
                    format!(
                        "## {}\nUpdate {} on {}\nNext schedule: {}",
                        t.channel_name, t.user_group_handle, t.cron, t.next_update_time
                    )
                })
                .collect()
        }
        Some(Command::New) => vec![format!("Show wizard to add new schedule")],
        None => vec![format!("default command")],
    };

    let sections = response_body
        .into_iter()
        .map(|p| format!(r#"{{"type": "section", "text": {{ "type": "mrkdwn", "text": "{}" }} }}"#, p))
        .collect::<Vec<String>>()
        .join(",\n");

    response(200, format!(r#"{{ "blocks": [{}] }}"#, sections))
}

fn parse_user_group(user_group: &str) -> Result<(String, String), AppError> {
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

    #[test]
    fn test_parse_slack_command_request_valid_full() {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&enterprise_id=E1234&enterprise_name=Acme&is_enterprise_install=true&user_id=U1234&user_name=john&command=/oncall&text=schedule&response_url=https://example.com/response";

        let result = parse_slack_command_request(request_body);
        assert!(result.is_ok());

        let params = result.unwrap();
        assert_eq!(params.team_id, "T1234");
        assert_eq!(params.team_domain, "example");
        assert_eq!(params.channel_id, "C1234");
        assert_eq!(params.channel_name, "general");
        assert_eq!(params.enterprise_id, "E1234");
        assert_eq!(params.enterprise_name, "Acme");
        assert_eq!(params.is_enterprise_install, "true");
        assert_eq!(params.user_id, "U1234");
        assert_eq!(params.user_name, "john");
        assert_eq!(params.command, "/oncall");
        assert_eq!(params.text, "schedule");
    }

    #[test]
    fn test_parse_slack_command_request_valid_minimal() {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&user_id=U1234&user_name=john&command=/oncall";

        let result = parse_slack_command_request(request_body);
        assert!(result.is_ok());

        let params = result.unwrap();
        assert_eq!(params.team_id, "T1234");
        assert_eq!(params.team_domain, "example");
        assert_eq!(params.channel_id, "C1234");
        assert_eq!(params.channel_name, "general");
        assert_eq!(params.enterprise_id, ""); // default value
        assert_eq!(params.enterprise_name, ""); // default value
        assert_eq!(params.is_enterprise_install, ""); // default value
        assert_eq!(params.user_id, "U1234");
        assert_eq!(params.user_name, "john");
        assert_eq!(params.command, "/oncall");
        assert_eq!(params.text, ""); // default value
    }

    #[test]
    fn test_parse_slack_command_request_with_empty_optional_fields() {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&enterprise_id=&enterprise_name=&is_enterprise_install=&user_id=U1234&user_name=john&command=/oncall&text=&response_url=";

        let result = parse_slack_command_request(request_body);
        assert!(result.is_ok());

        let params = result.unwrap();
        assert_eq!(params.team_id, "T1234");
        assert_eq!(params.enterprise_id, "");
        assert_eq!(params.enterprise_name, "");
        assert_eq!(params.is_enterprise_install, "");
        assert_eq!(params.text, "");
    }

    #[test]
    fn test_parse_slack_command_request_with_url_encoded_values() {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&user_id=U1234&user_name=john+doe&command=/oncall&text=schedule+--user-group+test";

        let result = parse_slack_command_request(request_body);
        assert!(result.is_ok());

        let params = result.unwrap();
        assert_eq!(params.user_name, "john doe");
        assert_eq!(params.text, "schedule --user-group test");
    }

    #[test]
    fn test_parse_slack_command_request_missing_required_field() {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&user_id=U1234";
        // Missing required fields: user_name, command

        let result = parse_slack_command_request(request_body);
        assert!(result.is_err());

        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("Failed to parse request body"));
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_parse_slack_command_request_empty_body() {
        let request_body = "";

        let result = parse_slack_command_request(request_body);
        assert!(result.is_err());

        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("Failed to parse request body"));
        } else {
            panic!("Expected InvalidData error");
        }
    }

    #[test]
    fn test_parse_slack_command_request_invalid_format() {
        let request_body = "not a valid urlencoded string!!!";

        let result = parse_slack_command_request(request_body);
        // This should actually succeed because serde_urlencoded is lenient
        // It will treat the whole string as a key with no value
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_slack_command_request_with_special_characters() {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&user_id=U1234&user_name=john&command=/oncall&text=%3C!subteam%5ES123%7C%40oncall%3E";

        let result = parse_slack_command_request(request_body);
        assert!(result.is_ok());

        let params = result.unwrap();
        assert_eq!(params.text, "<!subteam^S123|@oncall>");
    }

    #[test]
    fn test_parse_slack_command_request_enterprise_install_false() {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&is_enterprise_install=false&user_id=U1234&user_name=john&command=/oncall";

        let result = parse_slack_command_request(request_body);
        assert!(result.is_ok());

        let params = result.unwrap();
        assert_eq!(params.is_enterprise_install, "false");
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
    fn test_validate_request_valid() {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        // Create a valid signature
        let timestamp = chrono::Utc::now().timestamp();
        let sig_basestring = format!("v0:{}:{}", timestamp, request_body);
        let verification_key = hmac::Key::new(hmac::HMAC_SHA256, signing_secret.as_bytes());
        let signature = hex::encode(hmac::sign(&verification_key, sig_basestring.as_bytes()).as_ref());
        let expected_signature = format!("v0={}", signature);

        // Build headers
        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str(&expected_signature).unwrap());

        let result = validate_request(&headers, request_body, signing_secret);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_request_invalid_signature() {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        let timestamp = chrono::Utc::now().timestamp();
        let invalid_signature = "v0=invalid_signature_here";

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str(invalid_signature).unwrap());

        let result = validate_request(&headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Invalid slack command signature"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
    }

    #[test]
    fn test_validate_request_expired_timestamp() {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        // Use a timestamp that's 10 minutes old (should fail the 5 minute check)
        let timestamp = chrono::Utc::now().timestamp() - 600;
        let sig_basestring = format!("v0:{}:{}", timestamp, request_body);
        let verification_key = hmac::Key::new(hmac::HMAC_SHA256, signing_secret.as_bytes());
        let signature = hex::encode(hmac::sign(&verification_key, sig_basestring.as_bytes()).as_ref());
        let expected_signature = format!("v0={}", signature);

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str(&expected_signature).unwrap());

        let result = validate_request(&headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("wrong timestamp"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
    }

    #[test]
    fn test_validate_request_missing_timestamp_header() {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Signature", HeaderValue::from_str("v0=test").unwrap());

        let result = validate_request(&headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Missing X-Slack-Request-Timestamp header"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
    }

    #[test]
    fn test_validate_request_missing_signature_header() {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        let timestamp = chrono::Utc::now().timestamp();
        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());

        let result = validate_request(&headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Missing X-Slack-Signature header"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
    }

    #[test]
    fn test_validate_request_invalid_timestamp_format() {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str("not_a_number").unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str("v0=test").unwrap());

        let result = validate_request(&headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Invalid X-Slack-Request-Timestamp value"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
    }

    #[test]
    fn test_validate_request_signature_with_different_body() {
        let original_body = "token=test&team_id=T123&command=/oncall";
        let different_body = "token=test&team_id=T456&command=/oncall";
        let signing_secret = "test_secret";

        let timestamp = chrono::Utc::now().timestamp();
        let sig_basestring = format!("v0:{}:{}", timestamp, original_body);
        let verification_key = hmac::Key::new(hmac::HMAC_SHA256, signing_secret.as_bytes());
        let signature = hex::encode(hmac::sign(&verification_key, sig_basestring.as_bytes()).as_ref());
        let expected_signature = format!("v0={}", signature);

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str(&expected_signature).unwrap());

        // Validate with different body - should fail
        let result = validate_request(&headers, different_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Invalid slack command signature"));
        } else {
            panic!("Expected InvalidSlackRequest error");
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
