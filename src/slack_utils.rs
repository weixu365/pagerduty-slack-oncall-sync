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
