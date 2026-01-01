use std::env;

use lambda_http::{service_fn, Body, Error, Request, RequestExt, Response};
use on_call_support::{
    config::Config,
    http_util::response,
    logging,
    slack_handler::{handle_slack_command, handle_slack_oauth},
};
use tokio;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Initialize logging based on LOG_FORMAT environment variable
    logging::init_logging();

    info!("Starting Slack request handler Lambda");

    let service_func = service_fn(func);
    let result = lambda_http::run(service_func).await;

    match result {
        Ok(()) => {
            info!("Lambda execution completed successfully");
            Ok(())
        }
        Err(err) => {
            error!(error = %err, "Lambda execution failed");
            Err(err)
        }
    }
}

async fn func(request: Request) -> Result<Response<Body>, Error> {
    let env = env::var("ENV").unwrap_or("dev".to_string());
    let config = Config::get_or_init(&env).await?;

    let request_path = request.uri().path();
    let method = request.method().as_str();

    info!(method, request_path, "Received Slack request");

    match request_path {
        "/slack/oauth" => {
            info!("Processing Slack OAuth callback");
            // TODO: Return error if not allowed to install app based on ENV

            match handle_slack_oauth(&config, request.path_parameters()).await {
                Ok(res) => {
                    info!("Successfully processed Slack OAuth request");
                    Ok(res)
                }
                Err(err) => {
                    error!(%err, request_path, "Failed to process Slack OAuth request");
                    Err(err.into())
                }
            }
        }
        "/slack/command" => {
            let request_body = std::str::from_utf8(request.body().as_ref()).map_err(|e| {
                error!(error = %e, "Request body is not valid UTF-8");
                Error::from(format!("Request body is not valid UTF-8: {}", e))
            })?;

            info!("Processing Slack command");

            match handle_slack_command(&config, request.headers(), request_body).await {
                Ok(res) => {
                    info!("Successfully processed Slack command");
                    Ok(res)
                }
                Err(err) => {
                    error!(%err, request_path, "Failed to process Slack command");
                    Err(err.into())
                }
            }
        }
        _ => {
            warn!(method, request_path, "Received request for unknown path");
            response(400, format!("Invalid request")).map_err(|err| err.into())
        }
    }
}
