use aws_lambda_events::http::{HeaderMap, HeaderValue};

use lambda_http::Request;
use crate::{
    config::Config,
    encryptor::Encryptor,
    errors::AppError,
    utils::constant_time::constant_time_compare_str,
};
use clap::Parser;
use clap::{Args, Subcommand};
use lazy_static::lazy_static;
use regex::Regex;
use ring::hmac;
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct AppArgs {
    #[clap(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Args)]
pub struct ScheduleArgs {
    #[arg(long)]
    pub user_group: String,

    #[arg(long, default_value = "0 9 ? * MON-FRI *")]
    pub pagerduty_schedule: String,

    #[arg(long)]
    pub pagerduty_api_key: Option<String>,

    #[arg(long)]
    pub cron: String,

    #[arg(long)]
    pub timezone: Option<String>,
}

#[derive(Debug, Args)]
pub struct SetupPagerdutyArgs {
    #[arg(long)]
    pub pagerduty_api_key: String,
}

#[derive(Debug, Args)]
pub struct ListSchedulesArgs {
    #[arg(long)]
    all: Option<bool>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Schedule(ScheduleArgs),
    ListSchedules(ListSchedulesArgs),
    SetupPagerduty(SetupPagerdutyArgs),
    New,
}

#[derive(Debug, Deserialize)]
pub struct SlackCommandRequest {
    pub team_id: String,
    pub team_domain: String,
    pub channel_id: String,
    pub channel_name: String,
    #[serde(default)]
    pub enterprise_id: String,
    #[serde(default)]
    pub enterprise_name: String,
    #[serde(default)]
    pub is_enterprise_install: bool,
    pub user_id: String,
    pub user_name: String,
    pub command: String,
    #[serde(default)]
    pub text: String,
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

pub async fn parse_slack_request(
    request: Request,
    config: &Config,
) -> Result<(SlackCommandRequest, AppArgs), AppError> {
    let request_body = std::str::from_utf8(request.body().as_ref()).map_err(|e| {
        tracing::error!(error = %e, "Request body is not valid UTF-8");
        AppError::Error(format!("Request body is not valid UTF-8: {}", e))
    })?;

    let params = parse_slack_command_request(request_body)?;
    tracing::debug!(?params, "params in request body");

    let secrets = config.secrets().await?;
    validate_request(request.headers(), request_body, &secrets.slack_signing_secret)?;

    let arg = match shlex::split(cleanse(format!("{} {}", &params.command, &params.text).as_str()).as_str()) {
        Some(args) => Some(AppArgs::parse_from(args.iter())),
        None => None,
    };

    let encryptor = Encryptor::from_key(&secrets.encryption_key)?;

    let arg = arg.ok_or_else(|| AppError::InvalidData("Failed to parse command arguments".to_string()))?;

    Ok((params, arg))
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
        assert_eq!(params.is_enterprise_install, true);
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
        assert_eq!(params.is_enterprise_install, false); // default value
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
        assert_eq!(params.is_enterprise_install, false);
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
        assert_eq!(params.is_enterprise_install, false);
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

}
