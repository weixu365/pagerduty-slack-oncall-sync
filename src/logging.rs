use std::env;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

pub fn init_logging() {
    let log_format = env::var("LOG_FORMAT").unwrap_or_else(|_| "json".to_string());
    let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());

    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber_builder = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_span_events(FmtSpan::CLOSE)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false);
    
    match log_format.to_lowercase().as_str() {
        "json" => {
            subscriber_builder.json().init()
        }
        _ => {
            subscriber_builder.init()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_format_detection() {
        // Test that environment variables are correctly read
        env::set_var("LOG_FORMAT", "text");
        let format = env::var("LOG_FORMAT").unwrap();
        assert_eq!(format, "text");

        env::set_var("LOG_FORMAT", "json");
        let format = env::var("LOG_FORMAT").unwrap();
        assert_eq!(format, "json");
    }

    // Note: Cannot test init_logging() multiple times in the same process
    // as tracing subscriber can only be initialized once
}