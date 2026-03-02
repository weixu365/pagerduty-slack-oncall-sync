use std::{collections::HashMap, env, sync::Arc};

use crate::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    config::Config,
    db::{
        ScheduledTask, ScheduledTaskRepository, SlackInstallation, SlackInstallationRepository,
        dynamodb::{ScheduledTasksDynamodb, SlackInstallationsDynamoDb},
    },
    errors::AppError,
    service::{pager_duty::PagerDuty, slack::Slack},
    utils::{http_client::build_http_client, logging::json_tracing},
};
use futures::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResult {
    pub channel_id: String,
    pub channel_name: String,
    pub user_group_id: String,
    pub user_group_handle: String,
    pub original_user_ids: Vec<String>,
    pub new_user_ids: Vec<String>,
    pub changed: bool,
    pub error: Option<String>,
}

use chrono::{DateTime, Utc};
use reqwest::Client;

pub async fn update_user_group(
    http_client: Arc<Client>,
    pager_duty_api_key: &str,
    pager_duty_schedule_id: &str,
    pager_duty_schedule_from: DateTime<Utc>,
    slack_api_key: &str,
    slack_channel_id: &str,
    channel_name: &str,
    user_group_id: &str,
    user_group_handle: &str,
) -> Result<SyncResult, AppError> {
    json_tracing::info!("Getting the current on-call users", pager_duty_schedule_id, pager_duty_schedule_from);

    let from = pager_duty_schedule_from;

    let pager_duty = PagerDuty::new(http_client.clone(), pager_duty_api_key.into(), pager_duty_schedule_id.into());
    let oncall_users = pager_duty.get_on_call_users(Some(from)).await?;
    json_tracing::info!("Found users on call from PagerDuty", oncall_users, from);

    let slack = Arc::new(Slack::new(http_client.clone(), slack_api_key.to_string()));

    let new_slack_user_ids: Vec<String> = futures::stream::iter(oncall_users.into_iter())
        .map(|user| {
            let slack = slack.clone();
            let email = user.email;
            async move {
                slack
                    .get_user_by_email(&email)
                    .await?
                    .map(|u| u.id)
                    .ok_or_else(|| AppError::UnexpectedError(format!("Can't find Slack user by email: {}", email)))
            }
        })
        .buffer_unordered(5)
        .try_collect()
        .await?;

    let current_users = slack.get_user_group_users(user_group_id).await?;
    let current_user_names: Vec<String> = futures::stream::iter(&current_users)
        .then(|id| async {
            let id = id.clone();
            slack
                .get_user_by_id(&id)
                .await?
                .map(|u| u.name)
                .ok_or_else(|| AppError::UnexpectedError(format!("Can't find Slack user by id: {}", id)))
        })
        .try_collect()
        .await?;

    if current_users.len() > new_slack_user_ids.len() + 2 {
        json_tracing::error!(
            "Skipped: Too many users in the current Slack User Group",
            current_count = &current_users.len(),
            desired_count = &new_slack_user_ids.len(),
            user_group_id,
            user_group_handle,
        );
        return Err(AppError::SlackUpdateUserGroupError(
            "Too many users in the current group, is the group correct?".to_string(),
        ));
    }

    json_tracing::info!(
        "Current users in Slack User Group",
        user_ids = &current_users,
        user_names = &current_user_names
    );
    json_tracing::info!("On-call users in PagerDuty", user_ids = &new_slack_user_ids);

    let mut desired_users = new_slack_user_ids.clone();
    let mut existing_users = current_users.clone();
    desired_users.sort();
    existing_users.sort();

    let changed = desired_users != existing_users;
    json_tracing::info!("Does users changed", changed);

    if changed {
        slack
            .update_user_group_users(user_group_id, &new_slack_user_ids)
            .await?;

        let slack_users = new_slack_user_ids
            .iter()
            .map(|id| format!("<@{}>", id))
            .collect::<Vec<String>>()
            .join(", ");
        slack
            .send_message(
                slack_channel_id,
                &format!("Updated support user group <!subteam^{}> to: {}", user_group_id, slack_users),
            )
            .await?;
    }

    Ok(SyncResult {
        channel_id: slack_channel_id.to_string(),
        channel_name: channel_name.to_string(),
        user_group_id: user_group_id.to_string(),
        user_group_handle: user_group_handle.to_string(),
        original_user_ids: current_users,
        new_user_ids: new_slack_user_ids,
        changed,
        error: None,
    })
}

#[instrument(
    skip(task, slack_tokens, http_client, scheduled_tasks_db),
    fields(channel=task.channel_name, user_group=task.user_group_handle),
)]
async fn run_task(
    task: &ScheduledTask,
    slack_tokens: &HashMap<String, SlackInstallation>,
    http_client: Arc<Client>,
    scheduled_tasks_db: &ScheduledTasksDynamodb,
) -> Result<SyncResult, AppError> {
    json_tracing::info!("Updating user group", task_id = &task.task_id, cron = &task.cron);

    let slack_installation = slack_tokens
        .get(&task.team_id)
        .ok_or(AppError::SlackInstallationNotFoundError(format!(
            "Could not find slack installation for team: {}, task: {}",
            task.team, task.task_id
        )))?;

    let pagerduty_token = task
        .pager_duty_token
        .as_deref()
        .or(slack_installation.pager_duty_token.as_deref())
        .ok_or(AppError::SlackInstallationNotFoundError(format!(
            "No PagerDuty token setup for the current Slack installation, team: {}, task: {}",
            task.team, task.task_id
        )))?;

    let result = update_user_group(
        http_client.clone(),
        pagerduty_token,
        &task.pager_duty_schedule_id,
        Utc::now(),
        &slack_installation.access_token,
        &task.channel_id,
        &task.channel_name,
        &task.user_group_id,
        &task.user_group_handle,
    )
    .await?;

    let mut updated_task = task.clone();
    updated_task.last_updated_at = Utc::now().to_rfc3339();

    match updated_task.calculate_next_schedule(&Utc::now()) {
        Ok(task_next_schedule) => {
            updated_task.next_update_timestamp_utc = task_next_schedule.next_timestamp_utc;
            updated_task.next_update_time = task_next_schedule.next_datetime.to_rfc3339();
        }
        Err(err) => {
            updated_task.next_update_timestamp_utc = -1;
            updated_task.next_update_time = "".to_string();
            json_tracing::info!(
                "Failed to calculate next schedule",
                task_id = &task.task_id,
                cron = &task.cron,
                err = &err.to_string()
            );
        }
    }

    scheduled_tasks_db.update_next_schedule(&updated_task).await?;

    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncTrigger {
    Scheduled,
    Manual,
}

pub async fn update_user_groups(env: &str, trigger: SyncTrigger) -> Result<Vec<SyncResult>, AppError> {
    let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
    let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
    let config = Config::get_or_init(env).await?;
    let http_client = Arc::new(build_http_client()?);
    let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);
    let encryptor = config.build_encryptor().await?;

    let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
    let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor.clone());

    let slack_tokens: HashMap<String, SlackInstallation> = slack_installations_db
        .list_installations()
        .await?
        .into_iter()
        .map(|i| (i.team_id.clone(), i))
        .collect();

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    json_tracing::info!("Loaded tasks from DB", count = &tasks.len());

    let mut timestamp_of_next_trigger = i64::MAX;
    let mut next_task = None;
    let start_of_the_update = Utc::now();
    let mut results: Vec<SyncResult> = vec![];
    for task in tasks {
        let need_sync = task.next_update_timestamp_utc > 0 && task.next_update_timestamp_utc <= Utc::now().timestamp();
        if trigger == SyncTrigger::Manual || need_sync {
            let task_result = match run_task(&task, &slack_tokens, http_client.clone(), &scheduled_tasks_db).await {
                Ok(r) => r,
                Err(err) => {
                    json_tracing::error!(
                        "Failed to update user group for task",
                        task_id = &task.task_id,
                        err = &err.to_string()
                    );
                    SyncResult {
                        channel_id: task.channel_id.clone(),
                        channel_name: task.channel_name.clone(),
                        user_group_id: task.user_group_id.clone(),
                        user_group_handle: task.user_group_handle.clone(),
                        original_user_ids: vec![],
                        new_user_ids: vec![],
                        changed: false,
                        error: Some(err.to_string()),
                    }
                }
            };
            results.push(task_result);
        } else {
            json_tracing::info!(
                "Task skipped: next trigger is in the future",
                task_id = &task.task_id,
                next_update_time = &task.next_update_time,
                next_update_timestamp_utc = &task.next_update_timestamp_utc,
                now = &Utc::now().timestamp(),
            );
        }

        if task.next_update_timestamp_utc > start_of_the_update.timestamp()
            && task.next_update_timestamp_utc < timestamp_of_next_trigger
        {
            timestamp_of_next_trigger = task.next_update_timestamp_utc;
            next_task = Some(task);
        }
    }

    // at least re-run daily
    // (Utc::now() + Duration::days(1)).timestamp()
    if let Some(next) = next_task {
        json_tracing::info!(
            "Scheduling next update based on the next task",
            task_id = &next.task_id,
            cron = &next.cron,
            next_update_time = &next.next_update_time,
            next_update_timestamp_utc = &next.next_update_timestamp_utc,
            start_of_the_update,
        );

        match next.calculate_next_schedule(&start_of_the_update) {
            //TODO: if next schedule is earlier than now, re-run the above loop
            Ok(next_schedule) => {
                scheduler.update_next_schedule(&next_schedule).await?;
            }
            Err(err) => {
                json_tracing::error!(
                    "Failed to calculate next schedule",
                    task_id = &next.task_id,
                    cron = &next.cron,
                    err = &err.to_string()
                );
            }
        }
    }

    json_tracing::info!("Finished updating user groups");

    Ok(results)
}
