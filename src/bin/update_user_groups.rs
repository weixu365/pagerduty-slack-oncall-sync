use std::collections::HashMap;
use std::env;

use aws_sdk_cloudformation::Client as CloudformationClient;
use on_call_support::config::Config;
use on_call_support::errors::AppError;
use on_call_support::user_group_updater::update_user_groups;
use on_call_support::utils::logging::init_logging;
use tokio;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    init_logging();

    tracing::info!("Updating Slack user groups based on PagerDuty on-call schedule");
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

    update_user_groups(&env).await?;

    Ok(())
}
