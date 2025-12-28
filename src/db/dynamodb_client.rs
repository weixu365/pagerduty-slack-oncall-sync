use std::collections::HashMap;
use aws_sdk_dynamodb::types::AttributeValue;

pub fn get_attribute(item: &HashMap<String, AttributeValue>, name: &str) -> String {
    item
        .get(name)
        .and_then(|attr| 
            if attr.is_n() {
                attr.as_n().ok()
            } else {
                attr.as_s().ok()
            }
        )
        .expect(format!("field {} is null", name).as_str())
        .clone()
}

pub fn get_optional_attribute(item: &HashMap<String, AttributeValue>, name: &str) -> Option<String> {
    item
        .get(name)
        .and_then(|attr| 
            if attr.is_n() {
                attr.as_n().ok()
            } else {
                attr.as_s().ok()
            }
        )
        .map(|s| s.clone())
}
