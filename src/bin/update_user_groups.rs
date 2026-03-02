use std::collections::HashMap;
use std::env;

use aws_sdk_cloudformation::Client as CloudformationClient;
use on_call_support::config::Config;
use on_call_support::errors::AppError;
use on_call_support::user_group_updater::{SyncResult, SyncTrigger, update_user_groups};
use on_call_support::utils::logging::init_logging;
use on_call_support::utils::logging::json_tracing;
use tokio;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    if env::args().any(|a| a == "--version" || a == "-V") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    init_logging();

    let force = env::args().any(|a| a == "--force");
    let sync_trigger = if force {
        SyncTrigger::Manual
    } else {
        SyncTrigger::Scheduled
    };

    let version = env!("CARGO_PKG_VERSION");
    json_tracing::info!("Updating Slack user groups based on PagerDuty on-call schedule", version);
    let env = env::var("ENV").unwrap_or("dev".to_string());
    let config = Config::get_or_init(&env).await?;
    let cloudformation_stack_name = format!("on-call-support-{}", env);

    let cloudformation_client = CloudformationClient::new(&config.aws_config);
    let stack_details = cloudformation_client
        .describe_stacks()
        .stack_name(&cloudformation_stack_name)
        .send()
        .await?;
    let stack_outputs = &stack_details.stacks()[0].outputs.clone().unwrap_or(vec![]);
    let output_map: HashMap<String, String> = stack_outputs
        .iter()
        .filter_map(|output| {
            if let (Some(key), Some(value)) = (output.output_key.as_ref(), output.output_value.as_ref()) {
                Some((key.clone(), value.clone()))
            } else {
                None
            }
        })
        .collect();

    let lambda_arn = output_map
        .get("UpdateUserGroupsLambdaArn")
        .ok_or_else(|| AppError::UnexpectedError("UpdateUserGroupsLambdaArn not found".to_string()))?;
    let lambda_role_arn = output_map
        .get("UpdateUserGroupsLambdaRoleArn")
        .ok_or_else(|| AppError::UnexpectedError("UpdateUserGroupsLambdaRoleArn not found".to_string()))?;

    unsafe {
        env::set_var("UPDATE_USER_GROUP_LAMBDA", lambda_arn);
        env::set_var("UPDATE_USER_GROUP_LAMBDA_ROLE", lambda_role_arn);
    }

    let results = update_user_groups(&env, sync_trigger).await?;
    print_sync_summary(&results);

    Ok(())
}

fn print_sync_summary(results: &[SyncResult]) {
    let count = results.len();
    println!("");
    println!("## Summary");
    println!("Manual sync complete — {} schedule(s) processed", count);
    println!();

    for r in results {
        let to_users = r.new_user_ids.join(", ");
        let line = if let Some(ref err) = r.error {
            format!("- Channel #{}, User Group @{}: Error: {}", r.channel_name, r.user_group_handle, err)
        } else if r.changed {
            let from_users = r.original_user_ids.join(", ");
            format!(
                "- Channel #{}, User Group @{}: changed from [{}] to [{}]",
                r.channel_name, r.user_group_handle, from_users, to_users
            )
        } else {
            format!(
                "- Channel #{}, User Group @{}: no changes, user(s): [{}]",
                r.channel_name, r.user_group_handle, to_users
            )
        };
        println!("{}", line);
    }
}
