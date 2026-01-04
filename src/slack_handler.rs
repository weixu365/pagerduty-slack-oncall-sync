use aws_lambda_events::{
    encodings::Body,
    http::{HeaderMap, HeaderValue},
    query_map::QueryMap,
};
use lambda_http::Response;
use std::{collections::HashMap, env};

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
use form_urlencoded;
use lazy_static::lazy_static;
use regex::Regex;
use ring::hmac;
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

fn cleanse(text: &str) -> String {
    lazy_static! {
        static ref DOUBLE_QUOTES: Regex = Regex::new("[“”]").unwrap();
        static ref SINGLE_QUOTES: Regex = Regex::new("[‘’]").unwrap();
    }

    let cleansed_double_quote = DOUBLE_QUOTES.replace_all(text, "\"");
    let cleansed = SINGLE_QUOTES.replace_all(&cleansed_double_quote, "'");

    cleansed.to_string()
}

fn get_param(params: &HashMap<String, String>, name: &str) -> String {
    params.get(&name.to_string()).unwrap_or(&"".to_string()).to_string()
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
    let params: HashMap<String, String> = form_urlencoded::parse(request_body.as_bytes()).into_owned().collect();
    tracing::debug!(?params, "params in request body");

    let team_id = get_param(&params, "team_id");
    let team_domain = get_param(&params, "team_domain");
    let channel_id = get_param(&params, "channel_id");
    let channel_name = get_param(&params, "channel_name");
    let enterprise_id = get_param(&params, "enterprise_id");
    let enterprise_name = get_param(&params, "enterprise_name");
    let is_enterprise_install = get_param(&params, "is_enterprise_install").eq_ignore_ascii_case("true");

    let user_id = get_param(&params, "user_id");
    let user_name = get_param(&params, "user_name");
    let command = get_param(&params, "command");
    let text = get_param(&params, "text");
    let _response_url = get_param(&params, "response_url");

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
        return response(400, format!("Invalid slack command due to invalid timestamp: {} {}", command, text));
    }

    let sig_basestring = format!("v0:{}:{}", slack_request_timestamp, request_body);
    tracing::debug!(sig_basestring, "Slack Request to sign");

    let secrets = config.secrets().await?;
    let verification_key = hmac::Key::new(hmac::HMAC_SHA256, secrets.slack_signing_secret.as_bytes());
    let signature = hex::encode(hmac::sign(&verification_key, sig_basestring.as_bytes()).as_ref());

    let expected_signature = format!("v0={}", signature);

    if !constant_time_compare_str(&expected_signature, slack_request_signature) {
        tracing::error!(slack_request_signature, "Signature verification failed");
        return response(400, format!("Invalid slack command signature: {} {}", command, text));
    }

    let arg = match shlex::split(cleanse(format!("{} {}", command, text).as_str()).as_str()) {
        Some(args) => Some(App::parse_from(args.iter())),
        None => None,
    };

    let encryptor = Encryptor::from_key(&secrets.encryption_key)?;

    let arg = arg.ok_or_else(|| AppError::InvalidData("Failed to parse command arguments".to_string()))?;

    let response_body = match arg.command {
        Some(Command::Schedule(arg)) => {
            let user_group_id: String;
            let user_group_handle: String;

            let re = Regex::new(r"<!subteam\^(\w+)\|@([^>]+)>")?;
            if let Some(captures) = re.captures(arg.user_group.as_str()) {
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
                tracing::error!(user_group = %arg.user_group, "Invalid user group");

                return response(400, format!("Invalid user group: {}", arg.user_group));
            }

            let http_client = std::sync::Arc::new(build_http_client()?);

            let pagerduty_token = if let Some(ref token) = arg.pagerduty_api_key {
                token.clone()
            } else {
                let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
                slack_installations_db
                    .get_slack_installation(&team_id, &enterprise_id)
                    .await?
                    .pager_duty_token
                    .ok_or(AppError::SlackInstallationNotFoundError(format!(
                        "No PagerDuty token setup for the current Slack installation, team: {}",
                        team_id
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

            let task_id = format!(
                "{}:{}:{}:{}:{}",
                channel_name, channel_id, user_group_handle, user_group_id, arg.pagerduty_schedule
            );

            let task = ScheduledTask {
                team: format!("{}:{}", &team_id, &enterprise_id),
                task_id,
                next_update_timestamp_utc: next_schedule.next_timestamp_utc,
                next_update_time: next_schedule.next_datetime.to_rfc3339(),

                team_id,
                team_domain,
                channel_id,
                channel_name,
                enterprise_id,
                enterprise_name,
                is_enterprise_install,

                user_group_id,
                user_group_handle,
                pager_duty_schedule_id: arg.pagerduty_schedule,
                pager_duty_token: arg.pagerduty_api_key,
                cron: arg.cron,
                timezone: timezone.to_string(),

                created_by_user_id: user_id,
                created_by_user_name: user_name,
                created_at: Utc::now().to_rfc3339(),
                last_updated_at: Utc::now().to_rfc3339(),
            };

            if let Err(err) = db.save_scheduled_task(&task).await {
                tracing::error!(%err, "Failed to save to dynamodb");
                return response(500, format!("Failed to save schedule task to db\nCommand: {} {}", command, text));
            }

            if let Err(err) = scheduler.update_next_schedule(&next_schedule).await {
                tracing::error!(%err, "Failed to update scheduler");
                return response(500, format!("Failed to update scheduler\nCommand: {} {}", command, text));
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
                .update_pagerduty_token(team_id, enterprise_id, &args.pagerduty_api_key)
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
