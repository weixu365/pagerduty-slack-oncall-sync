use std::env;
use tracing_subscriber::EnvFilter;

pub fn init_logging() {
    let default_log_level = "warn,on_call_support=info,slack_request_handler_lambda=info,update_user_groups_lambda=info,update_user_groups=info";
    let log_format = env::var("LOG_FORMAT").unwrap_or_else(|_| "json".to_string());
    let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| default_log_level.to_string());

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    let subscriber_builder = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false);

    match log_format.to_lowercase().as_str() {
        "json" => subscriber_builder.json().flatten_event(true).init(),
        _ => subscriber_builder.init(),
    }
}
