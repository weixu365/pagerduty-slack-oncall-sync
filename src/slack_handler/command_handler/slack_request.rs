use aws_lambda_events::http::{HeaderMap, HeaderValue};
use tracing::warn;

use crate::{config::Config, errors::AppError, slack_handler::utils::request_utils::validate_request};
use clap::Parser;
use clap::{Args, Subcommand};
use lazy_static::lazy_static;
use regex::Regex;
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
    pub all: Option<bool>,

    #[arg(long)]
    pub page: Option<usize>,

    #[arg(long, default_value = "5")]
    pub page_size: usize,
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
    #[serde(default)]
    pub response_url: String,
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

pub async fn parse_slack_request(
    request_headers: HeaderMap<HeaderValue>,
    request_body: &str,
    config: &Config,
) -> Result<SlackCommandRequest, AppError> {
    let params = parse_slack_command_request(request_body)?;
    tracing::debug!(?params, "params in request body");

    let response_url = params.response_url.clone();
    if response_url.is_empty() {
        warn!("response_url is empty, cannot send response to Slack");
        return Err(AppError::InvalidSlackRequest(format!("response_url is empty")));
    }

    let secrets = config.secrets().await?;
    validate_request(request_headers, request_body, &secrets.slack_signing_secret)?;
    tracing::debug!(?params, "validated request");

    Ok(params)
}

pub async fn parse_slack_command(command: &str, args: &str) -> Result<AppArgs, AppError> {
    let arg = match shlex::split(cleanse(format!("{} {}", command, args).as_str()).as_str()) {
        Some(args) => Some(AppArgs::parse_from(args.iter())),
        None => None,
    };

    let arg = arg.ok_or_else(|| AppError::InvalidData("Failed to parse command arguments".to_string()))?;

    Ok(arg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_slack_command_request_valid_full() -> Result<(), AppError> {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&enterprise_id=E1234&enterprise_name=Acme&is_enterprise_install=true&user_id=U1234&user_name=john&command=/oncall&text=schedule&response_url=https://example.com/response";

        let params = parse_slack_command_request(request_body)?;

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
        Ok(())
    }

    #[test]
    fn test_parse_slack_command_request_valid_minimal() -> Result<(), AppError> {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&user_id=U1234&user_name=john&command=/oncall";

        let params = parse_slack_command_request(request_body)?;

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
        Ok(())
    }

    #[test]
    fn test_parse_slack_command_request_with_empty_optional_fields() -> Result<(), AppError> {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&enterprise_id=&enterprise_name=&user_id=U1234&user_name=john&command=/oncall&text=&response_url=";

        let params = parse_slack_command_request(request_body)?;

        assert_eq!(params.team_id, "T1234");
        assert_eq!(params.enterprise_id, "");
        assert_eq!(params.enterprise_name, "");
        assert_eq!(params.is_enterprise_install, false);
        assert_eq!(params.text, "");
        Ok(())
    }

    #[test]
    fn test_parse_slack_command_request_with_url_encoded_values() -> Result<(), AppError> {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&user_id=U1234&user_name=john+doe&command=/oncall&text=schedule+--user-group+test";

        let params = parse_slack_command_request(request_body)?;

        assert_eq!(params.user_name, "john doe");
        assert_eq!(params.text, "schedule --user-group test");
        Ok(())
    }

    #[test]
    fn test_parse_slack_command_request_missing_required_field() -> Result<(), AppError> {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&user_id=U1234";
        // Missing required fields: user_name, command

        let result = parse_slack_command_request(request_body);
        assert!(result.is_err());

        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("Failed to parse request body"));
        } else {
            panic!("Expected InvalidData error");
        }
        Ok(())
    }

    #[test]
    fn test_parse_slack_command_request_empty_body() -> Result<(), AppError> {
        let request_body = "";

        let result = parse_slack_command_request(request_body);
        assert!(result.is_err());

        if let Err(AppError::InvalidData(msg)) = result {
            assert!(msg.contains("Failed to parse request body"));
        } else {
            panic!("Expected InvalidData error");
        }
        Ok(())
    }

    #[test]
    fn test_parse_slack_command_request_invalid_format() -> Result<(), AppError> {
        let request_body = "not a valid urlencoded string!!!";

        let result = parse_slack_command_request(request_body);
        // This should actually succeed because serde_urlencoded is lenient
        // It will treat the whole string as a key with no value
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_parse_slack_command_request_with_special_characters() -> Result<(), AppError> {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&user_id=U1234&user_name=john&command=/oncall&text=%3C!subteam%5ES123%7C%40oncall%3E";

        let params = parse_slack_command_request(request_body)?;

        assert_eq!(params.text, "<!subteam^S123|@oncall>");
        Ok(())
    }

    #[test]
    fn test_parse_slack_command_request_enterprise_install_false() -> Result<(), AppError> {
        let request_body = "team_id=T1234&team_domain=example&channel_id=C1234&channel_name=general&is_enterprise_install=false&user_id=U1234&user_name=john&command=/oncall";

        let params = parse_slack_command_request(request_body)?;

        assert_eq!(params.is_enterprise_install, false);
        Ok(())
    }
}
