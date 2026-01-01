use std::env;

use on_call_support::{user_group_updater::update_user_groups, http_util::response};
use tokio;
use lambda_http::{Body, Error, Request, Response, service_fn};

use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let func = service_fn(func);
    lambda_http::run(func).await?;
    Ok(())
}

async fn func(_request: Request) -> Result<Response<Body>, Error> {
    let env = env::var("ENV").unwrap_or("dev".to_string());
    let result = update_user_groups(&env).await;

    match result {
        Ok(()) => {
            response(200, json!({ "message": "Updated user groups" }).to_string())
                .map_err(|err| err.into())
        },
        Err(err) => {
            tracing::error!(%err, "Failed to update user groups");
            Err(err.into())
        }
    }
}
