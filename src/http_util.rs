use lambda_http::{Body, Response};

use crate::errors::AppError;

pub fn response(status_code: u16, body: String) -> Result<Response<Body>, AppError> {
    Response::builder()
        .status(status_code)
        .body(Body::from(body))
        .map_err(|e| AppError::HttpError(format!("Failed to build response: {}", e)))
}
