use std::env;

use aws_config::{BehaviorVersion, SdkConfig};
use crate::{errors::AppError, secrets::{Secrets, SecretsClient}};

pub struct Config {
    pub env: String,

    pub cloudformation_stack_name: String,

    pub secrets: Secrets,

    pub schedules_table_name: String,
    pub installations_table_name: String,
    
    pub schedule_name_prefix: String,

    pub aws_config: SdkConfig,
}

impl Config {
    pub async fn new(env: &str) -> Result<Config, AppError> {
        tracing::debug!(env, "Loading config");

        let secret_name = env::var("AWS_SECRET_NAME").unwrap_or("on-call-support/secrets".to_string());
        let aws_config = ::aws_config::load_defaults(BehaviorVersion::latest()).await;
        let secrets_client = SecretsClient::new(&aws_config);
        let secrets = secrets_client.get_secret(&secret_name).await?;

        Ok(Config {
            env: env.to_string(),
            cloudformation_stack_name: format!("on-call-support-{}", env),
            secrets: secrets,
            schedules_table_name: format!("on-call-support-schedules-{}", env),
            installations_table_name: format!("on-call-support-installations-{}", env),

            schedule_name_prefix: "on-call-support-dev_UpdateUserGroupSchedule_".to_string(),

            aws_config: aws_config.clone(),
        })
    }
}
