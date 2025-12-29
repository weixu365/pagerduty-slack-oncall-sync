use on_call_support::{config::Config, http_util::response, slack_handler::{handle_slack_command, handle_slack_oauth}};
use tokio;
use lambda_http::{Body, Error, Request, RequestExt, Response, service_fn};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let service_func = service_fn(func);
    let result = lambda_http::run(service_func).await;
    
    match result {
        Ok(()) => Ok(()),
        Err(err) => {
            println!("Error occurred: {:?}", err);
            Err(err)
        }
    }    
}

async fn func(request: Request) -> Result<Response<Body>, Error> {
    let env = "dev";
    let config = Config::new(env).await?;
    
    let request_path = request.uri().path();
    match request_path {
        "/slack/oauth" => {
            // println!("Received Slack oauth request. request: {:?}", request);
            // TODO: Return error if not allowed to install app based on ENV

            match handle_slack_oauth(&config, request.path_parameters()).await  {
                Ok(res) => Ok(res),
                Err(err) => {
                    println!("Failed to process Slack OAuth request. err: {:?}", err);
                    Err(err.into())
                }
            }
        },
        "/slack/command" => {
            let request_body = std::str::from_utf8(request.body().as_ref())
                .map_err(|e| Error::from(format!("Request body is not valid UTF-8: {}", e)))?;
            // println!("Received Slack command. request_body: {:?}", request_body);

            match handle_slack_command(&config, request.headers(), request_body).await {
                Ok(res) => Ok(res),
                Err(err) => {
                    println!("Failed to process Slack command. err: {:?}", err);
                    Err(err.into())
                }
            }
        },
        _ => {
            println!("Ignored invalid request. request: {:?}", request);
            Ok(response(400, format!("Invalid request")))
        },
    }
}
