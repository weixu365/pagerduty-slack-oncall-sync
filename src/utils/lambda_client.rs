use std::env;
use std::sync::Arc;

use crate::{config::Config, errors::AppError};
use aws_lambda_events::event::apigw::ApiGatewayProxyRequest;
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_lambda::types::InvocationType;
use http::HeaderMap;
use reqwest::header::HeaderName;
use tracing::info;

pub async fn invoke_slack_command_async_handler(
    config: &Arc<Config>,
    event: ApiGatewayProxyRequest,
) -> Result<(), AppError> {
    info!("Received request from Slack, invoking lambda asynchronously");

    let lambda_client = LambdaClient::new(&config.aws_config);
    let slack_handler_arn = env::var("SLACK_HANDLER_LAMBDA_ARN")?;

    let mut async_event = event.clone();
    async_event
        .headers
        .insert(HeaderName::from_lowercase(b"x-slack-handler-async").unwrap(), "true".parse().unwrap());

    let payload = serde_json::to_vec(&async_event)?;
    lambda_client
        .invoke()
        .function_name(&slack_handler_arn)
        .invocation_type(InvocationType::Event)
        .payload(aws_sdk_lambda::primitives::Blob::new(payload))
        .send()
        .await?;

    info!("Lambda invoked asynchronously, returning acknowledgment to Slack");
    Ok(())
}

// Check if this is an async invocation (flag set when lambda invokes itself)
pub fn is_async_processing_requested(headers: &HeaderMap) -> bool {
    headers
        .get("x-slack-handler-async")
        .map(|v| v.to_str().unwrap_or("").to_lowercase() == "true")
        .unwrap_or(false)
}
