use crate::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    db::{ScheduledTask, ScheduledTaskRepository, SlackInstallationRepository},
    errors::AppError,
    service_provider::pager_duty::PagerDuty,
    slack_handler::command_handler::slack_request::{ScheduleArgs, SlackCommandRequest},
    utils::cron::get_next_schedule_from,
    utils::http_client::build_http_client,
};
use chrono::Utc;
use chrono_tz::Tz;
use regex::Regex;
use std::str::FromStr;

fn build_task_id(
    channel_name: &str,
    channel_id: &str,
    user_group_handle: &str,
    user_group_id: &str,
    pagerduty_schedule: &str,
) -> String {
    format!("{}:{}:{}:{}:{}", channel_name, channel_id, user_group_handle, user_group_id, pagerduty_schedule)
}

pub async fn handle_schedule_command(
    params: SlackCommandRequest,
    arg: ScheduleArgs,
    slack_installations_db: &dyn SlackInstallationRepository,
    scheduled_tasks_db: &dyn ScheduledTaskRepository,
    scheduler: EventBridgeScheduler,
) -> Result<Vec<String>, AppError> {
    let (user_group_id, user_group_handle) = parse_user_group(&arg.user_group)?;
    let http_client = std::sync::Arc::new(build_http_client()?);

    let pagerduty_token = if let Some(ref token) = arg.pagerduty_api_key {
        token.clone()
    } else {
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
    let pager_duty = PagerDuty::new(http_client.clone(), pagerduty_token.clone(), arg.pagerduty_schedule.clone());
    pager_duty.get_on_call_users(Utc::now()).await?;

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
        is_enterprise_install: params.is_enterprise_install,

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

    if let Err(err) = scheduled_tasks_db.save_scheduled_task(&task).await {
        tracing::error!(%err, "Failed to save to dynamodb");
        return Err(AppError::Error(format!("Failed to save schedule task\n{} {}", &params.command, &params.text)));
    }

    if let Err(err) = scheduler.update_next_schedule(&next_schedule).await {
        tracing::error!(%err, "Failed to update scheduler");
        return Err(AppError::Error(format!("Failed to update scheduler\n{} {}", &params.command, &params.text)));
    }

    Ok(vec![format!(
        "Update user group: {}|{} based on pagerduty schedule: {}, at: {}",
        task.user_group_id, task.user_group_handle, &task.pager_duty_schedule_id, &task.cron
    )])
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
