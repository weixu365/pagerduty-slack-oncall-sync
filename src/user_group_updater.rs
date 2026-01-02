use std::{collections::HashMap, env, sync::Arc};

use crate::{
    config::Config,
    db::{SlackInstallation, SlackInstallationsDynamoDb},
    encryptor::Encryptor,
    scheduled_tasks::{EventBridgeScheduler, ScheduledTask, ScheduledTasksDynamodb},
};
use futures::{StreamExt, TryStreamExt};
use tracing::{self, instrument};

use crate::{
    errors::AppError,
    http_client::build_http_client,
    service_provider::{pager_duty::PagerDuty, slack::Slack},
};
use chrono::{DateTime, Duration, Utc};
use reqwest::Client;

pub async fn update_user_group(
    http_client: Arc<Client>,
    pager_duty_api_key: &str,
    pager_duty_schedule_id: &str,
    pager_duty_schedule_from: DateTime<Utc>,
    slack_api_key: &str,
    slack_channel_id: &str,
    slack_user_group_name: &str,
) -> Result<(), AppError> {
    tracing::info!("Getting the current on-call users");

    let from = pager_duty_schedule_from;
    let until = from + Duration::minutes(10);

    let pager_duty = PagerDuty::new(http_client.clone(), pager_duty_api_key.into(), pager_duty_schedule_id.into());
    let oncall_users = pager_duty.get_on_call_users(from).await?;
    tracing::info!(?oncall_users, %from, %until, "Found users on call from PagerDuty");

    let slack = Arc::new(Slack::new(http_client.clone(), slack_api_key.to_string()));

    let user_group = slack.get_user_group(&slack_user_group_name).await?;
    tracing::info!(?user_group, "Found the user group from Slack");

    let slack_user_ids: Vec<String> =
        futures::stream::iter(oncall_users.into_iter())
            .map(|user| {
                let slack = slack.clone();
                let email = user.email;
                async move {
                    slack.get_user_by_email(&email).await?.map(|u| u.id).ok_or_else(|| {
                        AppError::UnexpectedError(format!("Can't find Slack user by email: {:?}", email))
                    })
                }
            })
            .buffer_unordered(5)
            .try_collect()
            .await?;

    let current_users = slack.get_user_group_users(&user_group.id).await?;
    let current_user_names: Vec<String> = futures::stream::iter(&current_users)
        .then(|user_id| async {
            let id = user_id.clone();
            slack
                .get_user_by_id(&id)
                .await?
                .map(|u| u.name)
                .ok_or_else(|| AppError::UnexpectedError(format!("Can't find Slack user by id: {:?}", id)))
        })
        .try_collect()
        .await?;

    if current_users.len() > slack_user_ids.len() + 2 {
        // send message to channel with message: failed to update user group due to too many users
        // return Err(AppError::SlackUpdateUserGroupError("Too many users in the current group, is the group correct?".to_string()));
    }

    tracing::info!(user_ids=?current_users, user_names=?current_user_names, "Current users in Slack User Group");
    tracing::info!(user_ids=?slack_user_ids, "On-call users in PagerDuty");

    let mut desired_users = slack_user_ids.clone();
    let mut existing_users = current_users.clone();
    desired_users.sort();
    existing_users.sort();

    let changed = desired_users != existing_users;
    tracing::info!(changed, "Does users changed");

    if changed {
        slack.update_user_group_users(&user_group.id, &slack_user_ids).await?;

        tracing::info!("Send message to Slack channel");
        let slack_users = slack_user_ids
            .iter()
            .map(|id| format!("<@{}>", id))
            .collect::<Vec<String>>()
            .join(", ");
        slack
            .send_message(
                &slack_channel_id,
                &format!("Updated support user group <!subteam^{}> to: {}", &user_group.id, slack_users),
            )
            .await?;
    }

    Ok(())
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
) -> Result<(), AppError> {
    tracing::info!(task_id = task.task_id, cron = task.cron, "Updating user group");

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

    update_user_group(
        http_client.clone(),
        pagerduty_token,
        &task.pager_duty_schedule_id,
        Utc::now(),
        &slack_installation.access_token,
        &task.channel_id,
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
            tracing::info!(task_id = task.task_id, cron = task.cron, %err, "Failed to calculate next schedule");
        }
    }

    scheduled_tasks_db.update_next_schedule(&updated_task).await?;

    Ok(())
}

pub async fn update_user_groups(env: &str) -> Result<(), AppError> {
    let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
    let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
    let config = Config::get_or_init(env).await?;
    let http_client = Arc::new(build_http_client()?);
    let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);
    let secrets = config.secrets().await?;
    let encryptor = Encryptor::from_key(&secrets.encryption_key)?;

    let slack_installations_db = SlackInstallationsDynamoDb::new(&config, encryptor.clone());
    let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor.clone());

    let slack_tokens: HashMap<String, SlackInstallation> = slack_installations_db
        .list_installations()
        .await?
        .into_iter()
        .map(|i| (i.team_id.clone(), i))
        .collect();

    let tasks = scheduled_tasks_db.list_scheduled_tasks().await?;
    tracing::info!(count = tasks.len(), "Loaded tasks from DB");

    let mut timestamp_of_next_trigger = i64::MAX;
    let mut next_task = None;
    let start_of_the_update = Utc::now();
    for task in tasks {
        if task.next_update_timestamp_utc > 0 && task.next_update_timestamp_utc <= Utc::now().timestamp() {
            let task_result = run_task(&task, &slack_tokens, http_client.clone(), &scheduled_tasks_db).await;
            if let Err(err) = task_result {
                tracing::error!(task_id=task.task_id, err=%err, "Failed to update user group for task");
            }
        } else {
            tracing::info!(
                task_id = task.task_id,
                next_update_time = task.next_update_time,
                next_update_timestamp_utc = task.next_update_timestamp_utc,
                now = Utc::now().timestamp(),
                "Task skipped: next trigger is in the future",
            );
        }

        if task.next_update_timestamp_utc > 0 && task.next_update_timestamp_utc < timestamp_of_next_trigger {
            timestamp_of_next_trigger = task.next_update_timestamp_utc;
            next_task = Some(task);
        }
    }

    // at least re-run daily
    // (Utc::now() + Duration::days(1)).timestamp()
    if let Some(next) = next_task {
        match next.calculate_next_schedule(&start_of_the_update) {
            //TODO: if next schedule is earlier than now, re-run the above loop
            Ok(next_schedule) => {
                scheduler.update_next_schedule(&next_schedule).await?;
            }
            Err(err) => {
                tracing::error!(task_id = next.task_id, cron = next.cron, %err, "Failed to calculate next schedule");
            }
        }
    }

    tracing::info!("Finished updating user groups");

    Ok(())
}
