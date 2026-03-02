use std::env;
use std::fmt;

use chrono::Utc;
use serde::Serialize;
use serde_json::{Map, Value, json};
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    EnvFilter,
    fmt::{
        FmtContext, FormatEvent, FormattedFields,
        format::{JsonFields, Writer},
    },
    registry::LookupSpan,
};

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
        "json" => subscriber_builder
            .json()
            .flatten_event(true)
            .fmt_fields(JsonFields::new())
            .event_format(JsonEventFormatter)
            .init(),
        _ => subscriber_builder.init(),
    }
}

pub fn to_json<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

struct JsonEventFormatter;

struct SmartJsonVisitor<'a>(&'a mut Map<String, Value>);

impl tracing::field::Visit for SmartJsonVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.0
            .insert(field.name().to_string(), try_parse_json(&format!("{value:?}")));
    }
}

fn try_parse_json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string()))
}

// Span fields are stored by JsonFields as bare "key":value pairs (no outer braces).
// Wrapping in {} makes it a valid JSON object we can parse directly.
fn span_fields<'a, N, S>(span: &tracing_subscriber::registry::SpanRef<'a, S>) -> Map<String, Value>
where
    N: for<'b> tracing_subscriber::fmt::FormatFields<'b> + 'static,
    S: tracing_subscriber::registry::LookupSpan<'a>,
{
    let ext = span.extensions();
    let Some(fields) = ext.get::<FormattedFields<N>>() else {
        return Map::new();
    };
    let s = fields.fields.as_str();
    if s.is_empty() {
        return Map::new();
    }
    serde_json::from_str::<Value>(s)
        .or_else(|_| serde_json::from_str::<Value>(&format!("{{{s}}}")))
        .ok()
        .and_then(|v| if let Value::Object(o) = v { Some(o) } else { None })
        .unwrap_or_default()
}

impl<S> FormatEvent<S, JsonFields> for JsonEventFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, JsonFields>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();

        let mut fields = Map::new();
        event.record(&mut SmartJsonVisitor(&mut fields));
        let message = fields
            .remove("message")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();

        let mut obj = json!({
            "timestamp": Utc::now().format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string(),
            "level": meta.level().to_string(),
            "message": message,
        })
        .as_object_mut()
        .unwrap()
        .clone();

        obj.extend(fields);
        obj.insert("target".into(), json!(meta.target()));

        // Span fields are stored by JsonFields as "key":value pairs in FormattedFields.
        // Wrapping in {} gives a valid JSON object.
        if let Some(scope) = ctx.event_scope() {
            let spans: Vec<Value> = scope
                .from_root()
                .map(|span| {
                    let mut span_obj = span_fields::<JsonFields, _>(&span);
                    span_obj.insert("name".into(), json!(span.name()));
                    Value::Object(span_obj)
                })
                .collect();

            if let Some(current) = spans.last().cloned() {
                obj.insert("span".into(), current);
            }
            if !spans.is_empty() {
                obj.insert("spans".into(), Value::Array(spans));
            }
        }

        writeln!(writer, "{}", serde_json::to_string(&obj).map_err(|_| fmt::Error)?)
    }
}

/// Log serializable fields as true nested JSON at the given tracing level.
///
/// Usage: `json_tracing::info!("message", field = &value, other = &other_value);`
/// Usage: `json_tracing::info!("message", field, other);`
/// Usage: `json_tracing::info!("message");`
#[macro_export]
macro_rules! json_tracing {
    ($level:ident, $msg:expr) => { tracing::$level!($msg) };
    ($level:ident, $msg:expr, $($field:ident $(= $value:expr)?),+ $(,)?) => {
        tracing::$level!(
            $(
                $field = %$crate::utils::logging::to_json($crate::json_tracing!(@value $field $(, $value)?)),
            )+
            $msg
        )
    };
    (@value $field:ident, $value:expr) => { $value };
    (@value $field:ident) => { &$field };
}

pub mod json_tracing {
    #[macro_export]
    macro_rules! _jt_debug { ($($t:tt)*) => { $crate::json_tracing!(debug, $($t)*) }; }
    #[macro_export]
    macro_rules! _jt_info  { ($($t:tt)*) => { $crate::json_tracing!(info,  $($t)*) }; }
    #[macro_export]
    macro_rules! _jt_warn  { ($($t:tt)*) => { $crate::json_tracing!(warn,  $($t)*) }; }
    #[macro_export]
    macro_rules! _jt_error { ($($t:tt)*) => { $crate::json_tracing!(error, $($t)*) }; }
    pub use crate::{_jt_debug as debug, _jt_error as error, _jt_info as info, _jt_warn as warn};
}
