use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

pub fn init_logging() {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_span_events(FmtSpan::CLOSE)
        .init();
}