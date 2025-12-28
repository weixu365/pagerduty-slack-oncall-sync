use on_call_support::{errors::AppError, slack_handler::{handle_slack_command, response, handle_slack_oauth}};
use tokio;
use lambda_http::{Body, Error, Request, RequestExt, Response, service_fn};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let result = lambda_http::run(service_fn(func)).await;
    
    match result {
        Ok(()) => Ok(()),
        Err(err) => {
            println!("Error occurred: {:?}", err);
            Err(err)
        }
    }    
}

async fn func(request: Request) -> Result<Response<Body>, AppError> {
    let env = "dev";

    let request_path = request.uri().path();
    match request_path {
        "/slack/oauth" => {
            // println!("Received Slack oauth request. request: {:?}", request);

            match handle_slack_oauth(env, request.path_parameters()).await  {
                Ok(res) => Ok(res),
                Err(err) => {
                    println!("Failed to process Slack OAuth request. err: {:?}", err);
                    Err(err)
                }
            }
        },
        "/slack/command" => {
            let request_body = str::from_utf8(request.body()).expect("Request Body is not valid UTF-8");
            // println!("Received Slack command. request_body: {:?}", request_body);

            match handle_slack_command(env, request.headers(), request_body).await {
                Ok(res) => Ok(res),
                Err(err) => {
                    println!("Failed to process Slack command. err: {:?}", err);
                    Err(err)
                }
            }
        },
        _ => {
            println!("Ignored invalid request. request: {:?}", request);
            Ok(response(400, format!("Invalid request")))
        },
    }
}
