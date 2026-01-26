use aws_lambda_events::{event::apigw::ApiGatewayProxyResponse, encodings::Body, http::HeaderMap};

use crate::errors::AppError;

pub fn response(status_code: i64, body: String) -> Result<ApiGatewayProxyResponse, AppError> {
    let mut response_headers = HeaderMap::new();
    // response_headers.insert("response_type", "in_channel".parse().unwrap());
    response_headers.insert("Content-type", "application/json".parse().unwrap());

    let mut response = ApiGatewayProxyResponse::default();
    response.status_code = status_code as i64;
    response.headers = response_headers.clone();
    response.body = Some(Body::from(body));
    
    Ok(response)
}


pub fn markdown_section(contents: Vec<String>) -> String {
    let sections = contents
        .into_iter()
        .map(|p| format!(r#"{{"type": "section", "text": {{ "type": "mrkdwn", "text": "{}" }} }}"#, p))
        .collect::<Vec<String>>()
        .join(",\n");

    let response_payload = format!(r#"{{ "blocks": [{}] }}"#, sections);
    
    response_payload
}
