use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct OptionsResponse {
    pub options: Vec<OptionItem>,
}

#[derive(Debug, Serialize)]
pub struct OptionItem {
    pub text: TextObject,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct TextObject {
    #[serde(rename = "type")]
    pub text_type: String,
    pub text: String,
}
