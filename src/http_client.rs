use reqwest::Client;

use crate::errors::AppError;

pub fn build_http_client() -> Result<Client, AppError> {
    let client = Client::builder()
        .build()?;

    Ok(client)
}
