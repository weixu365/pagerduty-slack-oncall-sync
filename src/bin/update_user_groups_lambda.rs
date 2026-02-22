use std::env;

use lambda_runtime::{Error, LambdaEvent, service_fn};
use on_call_support::{user_group_updater::{update_user_groups, SyncTrigger}, utils::logging};
use tokio;

use serde_json::{Value, json};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Error> {
    logging::init_logging();
    info!("Start updating Slack user groups based on PagerDuty on-call schedule");

    let func = service_fn(func);
    lambda_runtime::run(func).await?;
    Ok(())
}

async fn func(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let (event, _context) = event.into_parts();
    let env = env::var("ENV").unwrap_or("dev".to_string());
    let trigger = if event.get("manual").and_then(|v| v.as_bool()).unwrap_or(false) {
        SyncTrigger::Manual
    } else {
        SyncTrigger::Scheduled
    };
    let result = update_user_groups(&env, trigger).await;

    match result {
        Ok(results) => Ok(json!({ "results": results })),
        Err(err) => {
            tracing::error!(%err, "Failed to update user groups");
            Err(err.into())
        }
    }
}
