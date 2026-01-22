use aws_config::SdkConfig;
use aws_sdk_secretsmanager::Client;
use serde_derive::{Deserialize, Serialize};

use crate::errors::AppError;

#[derive(Serialize, Deserialize, Debug)]
pub struct Secrets {
    pub encryption_key: String,
    pub slack_client_id: String,
    pub slack_client_secret: String,
    pub slack_signing_secret: String,
}

pub struct SecretsClient {
    client: Client,
}

impl SecretsClient {
    pub fn new(config: &SdkConfig) -> SecretsClient {
        SecretsClient {
            client: Client::new(&config),
        }
    }

    pub async fn get_secret(&self, name: &str) -> Result<Secrets, AppError> {
        tracing::debug!(name, "Fetching secret value");

        let secrets_value = self.get_secret_value(name).await?;
        let secrets: Secrets = serde_json::from_str(&secrets_value)?;
        tracing::debug!(name, "Fetched secret value");

        Ok(secrets)
    }

    pub async fn get_secret_value(&self, name: &str) -> Result<String, AppError> {
        tracing::debug!(name, "Getting secret value");

        let result = self.client.get_secret_value().secret_id(name).send().await?;

        let secrets_value = result
            .secret_string()
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::InvalidSecret(format!("secret {} doesn't exist", name)))?;
        Ok(secrets_value)
    }
}
