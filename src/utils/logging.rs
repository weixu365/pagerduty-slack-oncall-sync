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

use serde::Serialize;

pub fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

/// Log a struct as a JSON string that CloudWatch can search
///
/// The struct must implement `serde::Serialize`.
///
/// Usage: `json_tracing::info!("message", field_name = &my_struct, another = &other);`
#[macro_export]
macro_rules! json_tracing_info {
    ($msg:expr, $($field:tt = $structure:expr),+ $(,)?) => {
        tracing::info!(
            $($field = $crate::utils::logging::to_json($structure),)+
            $msg
        )
    };
}

/// Log a struct as a JSON string at warn level
#[macro_export]
macro_rules! json_tracing_warn {
    ($msg:expr, $($field:tt = $structure:expr),+ $(,)?) => {
        tracing::warn!(
            $($field = $crate::utils::logging::to_json($structure),)+
            $msg
        )
    };
}

/// Log a struct as a JSON string at error level
#[macro_export]
macro_rules! json_tracing_error {
    ($msg:expr, $($field:tt = $structure:expr),+ $(,)?) => {
        tracing::error!(
            $($field = $crate::utils::logging::to_json($structure),)+
            $msg
        )
    };
}

/// Log a struct as a JSON string at debug level
#[macro_export]
macro_rules! json_tracing_debug {
    ($msg:expr, $($field:tt = $structure:expr),+ $(,)?) => {
        tracing::debug!(
            $($field = $crate::utils::logging::to_json($structure),)+
            $msg
        )
    };
}

pub mod json_tracing {
    // Re-export the macros so they can be used as json_tracing::info!
    pub use crate::json_tracing_debug as debug;
    pub use crate::json_tracing_error as error;
    pub use crate::json_tracing_info as info;
    pub use crate::json_tracing_warn as warn;
}
