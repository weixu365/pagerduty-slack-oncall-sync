use crate::{encryptor::Encryptor, errors::AppError};
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

pub fn get_optional_attribute(item: &HashMap<String, AttributeValue>, name: &str) -> Option<String> {
    item.get(name)
        .and_then(|attr| {
            if attr.is_n() {
                attr.as_n().ok()
            } else {
                attr.as_s().ok()
            }
        })
        .map(|s| s.clone())
}

pub fn get_attribute(item: &HashMap<String, AttributeValue>, name: &str) -> Result<String, AppError> {
    get_optional_attribute(item, name)
        .ok_or_else(|| AppError::UnexpectedError(format!("Missing or invalid field '{}'", name)))
}

pub fn get_optional_encrypted_attribute(
    item: &HashMap<String, AttributeValue>,
    name: &str,
    encryptor: &Encryptor,
) -> Result<Option<String>, AppError> {
    get_optional_attribute(item, name)
        .map(|encrypted| {
            let encrypted_token = serde_json::from_str(&encrypted)
                .map_err(|e| AppError::InvalidData(format!("invalid json field {}. Error: {}", name, e)))?;

            let decrypted = encryptor
                .decrypt(&encrypted_token)
                .map_err(|e| AppError::InvalidData(format!("can't decrypt field {}. Error: {}", name, e)))?;

            Ok(decrypted.clone())
        })
        .transpose()
}

pub fn get_encrypted_attribute(
    item: &HashMap<String, AttributeValue>,
    name: &str,
    encryptor: &Encryptor,
) -> Result<String, AppError> {
    get_optional_encrypted_attribute(item, name, encryptor)?
        .ok_or_else(|| AppError::UnexpectedError(format!("Missing or invalid field '{}'", name)))
}
