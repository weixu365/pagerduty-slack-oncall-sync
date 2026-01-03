use std::env;
use tracing_subscriber::EnvFilter;

pub fn init_logging() {
    let log_format = env::var("LOG_FORMAT").unwrap_or_else(|_| "json".to_string());
    let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "warn,on_call_support=info".to_string());

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
