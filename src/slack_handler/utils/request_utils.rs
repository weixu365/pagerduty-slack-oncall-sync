use aws_lambda_events::http::{HeaderMap, HeaderValue};

use crate::utils::logging::json_tracing;
use crate::{errors::AppError, utils::constant_time::constant_time_compare_str};

use ring::hmac;

pub fn validate_request(
    request_headers: HeaderMap<HeaderValue>,
    request_body: &str,
    slack_signing_secret: &str,
) -> Result<(), AppError> {
    let slack_request_timestamp = request_headers
        .get("X-Slack-Request-Timestamp")
        .ok_or_else(|| AppError::InvalidSlackRequest("Missing X-Slack-Request-Timestamp header".to_string()))?
        .to_str()
        .map_err(|_| AppError::InvalidSlackRequest("Invalid X-Slack-Request-Timestamp encoding".to_string()))?
        .parse::<i64>()
        .map_err(|_| AppError::InvalidSlackRequest("Invalid X-Slack-Request-Timestamp value".to_string()))?;

    let slack_request_signature = request_headers
        .get("X-Slack-Signature")
        .ok_or_else(|| AppError::InvalidSlackRequest("Missing X-Slack-Signature header".to_string()))?
        .to_str()
        .map_err(|_| AppError::InvalidSlackRequest("Invalid X-Slack-Signature encoding".to_string()))?;

    let now = chrono::Utc::now().timestamp();
    if (now - slack_request_timestamp).abs() > 60 * 5 {
        return Err(AppError::InvalidSlackRequest(format!("Invalid slack command: wrong timestamp")));
    }

    let sig_basestring = format!("v0:{}:{}", slack_request_timestamp, request_body);
    json_tracing::debug!("Slack Request to sign", sig_basestring);

    let verification_key = hmac::Key::new(hmac::HMAC_SHA256, slack_signing_secret.as_bytes());
    let signature = hex::encode(hmac::sign(&verification_key, sig_basestring.as_bytes()).as_ref());
    let expected_signature = format!("v0={}", signature);

    if !constant_time_compare_str(&expected_signature, slack_request_signature) {
        json_tracing::error!("Signature verification failed", slack_request_signature);
        return Err(AppError::InvalidSlackRequest(format!("Invalid slack command signature")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_request_valid() -> Result<(), AppError> {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        let timestamp = chrono::Utc::now().timestamp();
        let sig_basestring = format!("v0:{}:{}", timestamp, request_body);
        let verification_key = hmac::Key::new(hmac::HMAC_SHA256, signing_secret.as_bytes());
        let signature = hex::encode(hmac::sign(&verification_key, sig_basestring.as_bytes()).as_ref());
        let expected_signature = format!("v0={}", signature);

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str(&expected_signature).unwrap());

        validate_request(headers, request_body, signing_secret)?;
        Ok(())
    }

    #[test]
    fn test_validate_request_invalid_signature() -> Result<(), AppError> {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        let timestamp = chrono::Utc::now().timestamp();
        let invalid_signature = "v0=invalid_signature_here";

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str(invalid_signature).unwrap());

        let result = validate_request(headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Invalid slack command signature"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
        Ok(())
    }

    #[test]
    fn test_validate_request_expired_timestamp() -> Result<(), AppError> {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        // Use a timestamp that's 10 minutes old (should fail the 5 minute check)
        let timestamp = chrono::Utc::now().timestamp() - 600;
        let sig_basestring = format!("v0:{}:{}", timestamp, request_body);
        let verification_key = hmac::Key::new(hmac::HMAC_SHA256, signing_secret.as_bytes());
        let signature = hex::encode(hmac::sign(&verification_key, sig_basestring.as_bytes()).as_ref());
        let expected_signature = format!("v0={}", signature);

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str(&expected_signature).unwrap());

        let result = validate_request(headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("wrong timestamp"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
        Ok(())
    }

    #[test]
    fn test_validate_request_missing_timestamp_header() -> Result<(), AppError> {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Signature", HeaderValue::from_str("v0=test").unwrap());

        let result = validate_request(headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Missing X-Slack-Request-Timestamp header"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
        Ok(())
    }

    #[test]
    fn test_validate_request_missing_signature_header() -> Result<(), AppError> {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        let timestamp = chrono::Utc::now().timestamp();
        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());

        let result = validate_request(headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Missing X-Slack-Signature header"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
        Ok(())
    }

    #[test]
    fn test_validate_request_invalid_timestamp_format() -> Result<(), AppError> {
        let request_body = "token=test&team_id=T123&command=/oncall";
        let signing_secret = "test_secret";

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str("not_a_number").unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str("v0=test").unwrap());

        let result = validate_request(headers, request_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Invalid X-Slack-Request-Timestamp value"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
        Ok(())
    }

    #[test]
    fn test_validate_request_signature_with_different_body() -> Result<(), AppError> {
        let original_body = "token=test&team_id=T123&command=/oncall";
        let different_body = "token=test&team_id=T456&command=/oncall";
        let signing_secret = "test_secret";

        let timestamp = chrono::Utc::now().timestamp();
        let sig_basestring = format!("v0:{}:{}", timestamp, original_body);
        let verification_key = hmac::Key::new(hmac::HMAC_SHA256, signing_secret.as_bytes());
        let signature = hex::encode(hmac::sign(&verification_key, sig_basestring.as_bytes()).as_ref());
        let expected_signature = format!("v0={}", signature);

        let mut headers = HeaderMap::new();
        headers.insert("X-Slack-Request-Timestamp", HeaderValue::from_str(&timestamp.to_string()).unwrap());
        headers.insert("X-Slack-Signature", HeaderValue::from_str(&expected_signature).unwrap());

        // Validate with different body - should fail
        let result = validate_request(headers, different_body, signing_secret);
        assert!(result.is_err());

        if let Err(AppError::InvalidSlackRequest(msg)) = result {
            assert!(msg.contains("Invalid slack command signature"));
        } else {
            panic!("Expected InvalidSlackRequest error");
        }
        Ok(())
    }
}
