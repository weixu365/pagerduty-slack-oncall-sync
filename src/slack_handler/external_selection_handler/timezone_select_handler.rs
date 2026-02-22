use chrono::Offset;
use chrono::Utc;

use crate::utils::logging::json_tracing;
use crate::{
    errors::AppError,
    slack_handler::external_selection_handler::{
        options::{OptionItem, OptionsResponse, TextObject},
        slack_request::ExternalSelectRequest,
    },
};

const MAX_OPTIONS: usize = 100;

pub async fn handle_timezone_options(request: &ExternalSelectRequest) -> Result<OptionsResponse, AppError> {
    json_tracing::info!("Fetching timezone options", action_id = &request.action_id);

    let now = Utc::now();

    let mut timezones: Vec<(String, String, String)> = chrono_tz::TZ_VARIANTS
        .iter()
        .map(|tz| {
            let offset_seconds = now.with_timezone(tz).offset().fix().local_minus_utc();
            let offset_hours = offset_seconds / 3600;
            let offset_minutes = (offset_seconds % 3600).abs() / 60;
            let offset = format!("UTC{:>+03}:{:02}", offset_hours, offset_minutes);
            let name = tz.name().to_string();
            let label = format!("{} ({})", tz.name(), offset);
            (offset, name, label)
        })
        .collect();

    if let Some(search_value) = request.value.as_deref() {
        let search_lower = search_value.to_lowercase();
        timezones.retain(|(_, _, label)| label.to_lowercase().contains(&search_lower));
    }

    timezones.sort_by(|(offset_a, name_a, _), (offset_b, name_b, _)| {
        offset_a.cmp(offset_b).then_with(|| name_a.cmp(name_b))
    });

    let options = timezones
        .into_iter()
        .take(MAX_OPTIONS)
        .map(|(_, name, label)| OptionItem {
            text: TextObject {
                text_type: "plain_text".to_string(),
                text: label,
            },
            value: name,
        })
        .collect();

    Ok(OptionsResponse { options })
}
