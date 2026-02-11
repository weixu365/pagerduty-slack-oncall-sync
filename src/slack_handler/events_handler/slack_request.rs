use crate::errors::AppError;
use crate::slack_handler::morphism_patches::push_event::SlackPushEvent;

pub fn parse_slack_request(request_body: &str) -> Result<SlackPushEvent, AppError> {
    let request: SlackPushEvent = serde_json::from_str(request_body)
        .map_err(|e| AppError::InvalidData(format!("Failed to parse payload JSON: {}", e)))?;

    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_slack_command_request_invalid_format() -> Result<(), AppError> {
        let request_body = "not a valid urlencoded string!!!";

        let result = parse_slack_request(request_body);
        // This should actually succeed because serde_urlencoded is lenient
        // It will treat the whole string as a key with no value
        assert!(result.is_err());
        Ok(())
    }
}
