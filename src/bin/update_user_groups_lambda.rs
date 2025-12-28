use on_call_support::user_group_updater::update_user_groups;
use tokio;

use lambda_runtime::{service_fn, LambdaEvent, Error};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let func = service_fn(func);
    lambda_runtime::run(func).await?;
    Ok(())
}

async fn func(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let (_event, _context) = event.into_parts();
    let env = "dev";
    let result = update_user_groups(env).await;

    match result {
        Ok(()) => Ok(json!({ "message": "Updated user groups" })),
        Err(err) => {
            println!("Failed to update user groups: {:?}", err);
            Err(Box::new(err))
        }
    }
}
