use crate::{encryptor::Encryptor, errors::AppError};
use aws_sdk_dynamodb::types::AttributeValue;
use std::{collections::HashMap, sync::Arc};

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

pub async fn get_optional_encrypted_attribute(
    item: &HashMap<String, AttributeValue>,
    name: &str,
    encryptor: &Arc<dyn Encryptor + Send + Sync>,
) -> Result<Option<String>, AppError> {
    let encrypted = match get_optional_attribute(item, name) {
        None => return Ok(None),
        Some(v) => v,
    };

    if encrypted.is_empty() {
        return Ok(None);
    }

    let decrypted = encryptor
        .decrypt(&encrypted)
        .await
        .map_err(|e| AppError::InvalidData(format!("can't decrypt field {}. Error: {}", name, e)))?;

    Ok(Some(decrypted))
}

pub async fn get_encrypted_attribute(
    item: &HashMap<String, AttributeValue>,
    name: &str,
    encryptor: &Arc<dyn Encryptor + Send + Sync>,
) -> Result<String, AppError> {
    get_optional_encrypted_attribute(item, name, encryptor).await?
        .ok_or_else(|| AppError::UnexpectedError(format!("Missing or invalid field '{}'", name)))
}
