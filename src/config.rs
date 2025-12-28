use std::env;

use aws_config::{SdkConfig};
use crate::{errors::AppError, secrets::{Secrets, SecretsClient}};

pub struct Config {
    pub secret_name: String,
    pub secrets: Secrets,

    pub schedules_table_name: String,
    pub installations_table_name: String,
    
    pub schedule_name_prefix: String,
}

impl Config {
    pub async fn new(env: &str, aws_config: &SdkConfig) -> Result<Config, AppError> {
        let secret_name = env::var("AWS_SECRET_NAME")
            .expect("AWS_SECRET_NAME must be set and the value should contains encryption_key, slack_client_id, slack_client_secret, slack_signing_secret in json format");
        
        let secrets_client = SecretsClient::new(&aws_config);
        let secrets = secrets_client.get_secret(&secret_name).await?;

        Ok(Config {
            secret_name: "on-call-support/secrets".to_string(),
            secrets: secrets,
            schedules_table_name: format!("on-call-support-schedules-{}", env),
            installations_table_name: format!("on-call-support-installations-{}", env),

            schedule_name_prefix: "on-call-support-dev_UpdateUserGroupSchedule_".to_string(),
        })
    }
}
