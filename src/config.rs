use std::env;
use std::sync::Arc;
use tokio::sync::OnceCell;

use crate::{
    errors::AppError,
    secrets::{Secrets, SecretsClient},
};
use aws_config::{BehaviorVersion, SdkConfig};

pub struct Config {
    pub env: String,

    pub cloudformation_stack_name: String,

    pub schedules_table_name: String,
    pub installations_table_name: String,

    pub schedule_name_prefix: String,

    pub aws_config: SdkConfig,

    secrets_cache: OnceCell<Secrets>,
    secret_name: String,
}

static CONFIG_CACHE: OnceCell<Arc<Config>> = OnceCell::const_new();

impl Config {
    pub async fn get_or_init(env: &str) -> Result<Arc<Config>, AppError> {
        CONFIG_CACHE
            .get_or_try_init(|| async {
                tracing::info!(env, "Initializing config (first time in this container)");
                let config = Self::load(env).await?;
                Ok(Arc::new(config))
            })
            .await
            .map(Arc::clone)
    }

    async fn load(env: &str) -> Result<Config, AppError> {
        tracing::debug!(env, "Loading config from AWS");

        let secret_name = env::var("AWS_SECRET_NAME").unwrap_or("on-call-support/secrets".to_string());
        let aws_config = ::aws_config::load_defaults(BehaviorVersion::latest()).await;

        Ok(Config {
            env: env.to_string(),
            cloudformation_stack_name: format!("on-call-support-{}", env),
            schedules_table_name: format!("on-call-support-schedules-{}", env),
            installations_table_name: format!("on-call-support-installations-{}", env),

            schedule_name_prefix: "on-call-support-dev_UpdateUserGroupSchedule_".to_string(),

            aws_config: aws_config.clone(),
            secrets_cache: OnceCell::new(),
            secret_name,
        })
    }

    pub async fn secrets(&self) -> Result<&Secrets, AppError> {
        self.secrets_cache
            .get_or_try_init(|| async {
                tracing::info!(secret_name = %self.secret_name, "Loading secrets from AWS Secrets Manager");
                let secrets_client = SecretsClient::new(&self.aws_config);
                secrets_client.get_secret(&self.secret_name).await
            })
            .await
    }
}
