use std::collections::HashMap;
use std::env;

use aws_config::BehaviorVersion;
use on_call_support::errors::AppError;
use on_call_support::user_group_updater::update_user_groups;
use aws_sdk_cloudformation::Client as CloudformationClient;
use tokio;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let cloudformation_stack_name = "on-call-support-dev";

    let config = ::aws_config::load_defaults(BehaviorVersion::latest()).await;
    let cloudformation_client = CloudformationClient::new(&config);
    let stack_details = cloudformation_client.describe_stacks().stack_name(cloudformation_stack_name).send().await?;
    let stack_outputs = &stack_details.stacks()[0].outputs.clone().unwrap_or(vec![]);
    let output_map: HashMap<String, String> = stack_outputs.iter().filter_map(|output| {
        if let (Some(key), Some(value)) = (output.output_key.as_ref(), output.output_value.as_ref()) {
            Some((key.clone(), value.clone()))
        } else {
            None
        }
    }).collect();

    let lambda_arn = output_map.get("UpdateUserGroupsLambdaArn")
        .expect("UpdateUserGroupsLambdaArn not found");
    let lambda_role_arn = output_map.get("UpdateUserGroupsLambdaRoleArn")
        .expect("UpdateUserGroupsLambdaRoleArn not found");

    env::set_var("UPDATE_USER_GROUP_LAMBDA", lambda_arn);
    env::set_var("UPDATE_USER_GROUP_LAMBDA_ROLE", lambda_role_arn);

    update_user_groups("dev").await?;
    
    Ok(())
}
