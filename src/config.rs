use std::env;
use std::sync::Arc;
use tokio::sync::OnceCell;

use crate::{
    aws::secrets_client::{Secrets, SecretsClient},
    encryptor::{AWSKMSEncryptor, Encryptor, XChaCha20Encryptor},
    errors::AppError,
};
use aws_config::{BehaviorVersion, SdkConfig};
use aws_sdk_kms::Client as KmsClient;

static CONFIG_CACHE: OnceCell<Arc<Config>> = OnceCell::const_new();

pub struct Config {
    pub env: String,

    pub schedules_table_name: String,
    pub installations_table_name: String,

    pub schedule_name_prefix: String,

    pub aws_config: SdkConfig,

    secrets_cache: OnceCell<Secrets>,
    secret_name: String,
}

impl Config {
    pub async fn get_or_init(env: &str) -> Result<Arc<Config>, AppError> {
        let config = CONFIG_CACHE
            .get_or_try_init(|| async {
                tracing::info!(env, "Loading config for env");
                Self::load(env).await
            })
            .await?;

        Ok(config.clone())
    }

    async fn load(env: &str) -> Result<Arc<Config>, AppError> {
        tracing::info!(env, "Loading config");

        let secret_name = env::var("AWS_SECRET_NAME").unwrap_or("on-call-support/secrets".to_string());
        let table_name_prefix = env::var("TABLE_NAME_PREFIX").unwrap_or("on-call-support-".to_string());
        let schedule_name_prefix = env::var("SCHEDULE_NAME_PREFIX").unwrap_or("on-call-support-".to_string());
        let aws_config = ::aws_config::load_defaults(BehaviorVersion::latest()).await;

        Ok(Arc::new(Config {
            env: env.to_string(),
            schedules_table_name: format!("{}schedules-{}", table_name_prefix, env),
            installations_table_name: format!("{}installations-{}", table_name_prefix, env),
            schedule_name_prefix: format!("{}{}_UpdateUserGroupSchedule_", schedule_name_prefix, env),

            aws_config,
            secrets_cache: OnceCell::new(),
            secret_name,
        }))
    }

    pub async fn secrets(&self) -> Result<&Secrets, AppError> {
        tracing::info!(secret_name = %self.secret_name, "Loading secrets");
        let result = self.secrets_cache
            .get_or_try_init(|| async {
                tracing::info!(secret_name = %self.secret_name, "Loading secrets from AWS Secrets Manager");
                let secrets_client = SecretsClient::new(&self.aws_config);
                secrets_client.get_secret(&self.secret_name).await

            })
            .await;

        tracing::info!(secret_name = %self.secret_name, "Loaded secrets");
        result
    }

    pub async fn build_encryptor(&self) -> Result<Arc<dyn Encryptor + Send + Sync>, AppError> {
        let kms_key_id = env::var("KMS_KEY_ID").ok();
        let secret_id = env::var("AWS_SECRET_ID").ok();

        if let Some(kms_key_id) = kms_key_id {
            tracing::info!(kms_key_id = %kms_key_id, "Using AWS KMS encryption");
            let kms_client = KmsClient::new(&self.aws_config);
            let encryptor = AWSKMSEncryptor::new(kms_client, kms_key_id).await?;
            Ok(Arc::new(encryptor))
        } else if let Some(secret_id) = secret_id {
            tracing::info!(aws_secret_id = %secret_id, "Using XChaCha20 encryption with key from AWS Secret");
            let secrets_client = SecretsClient::new(&self.aws_config);
            let encryption_key = secrets_client.get_secret_value(&secret_id).await?;

            let encryptor = XChaCha20Encryptor::from_key(&encryption_key)?;
            Ok(Arc::new(encryptor))
        } else {
            tracing::info!("Using XChaCha20 encryption");
            let secrets = self.secrets().await?;
            let encryptor = XChaCha20Encryptor::from_key(&secrets.encryption_key)?;
            Ok(Arc::new(encryptor))
        }
    }
}
