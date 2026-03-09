use std::env;
use std::sync::Arc;
use tokio::sync::OnceCell;

use crate::utils::logging::json_tracing;
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

    pub admin_user_slack_ids: Vec<String>,

    pub aws_config: SdkConfig,

    secrets_cache: OnceCell<Secrets>,
    secret_name: String,
}

impl Config {
    pub async fn get_or_init(env: &str) -> Result<Arc<Config>, AppError> {
        let config = CONFIG_CACHE
            .get_or_try_init(|| async {
                json_tracing::info!("Loading config for env", env);
                Self::load(env).await
            })
            .await?;

        Ok(config.clone())
    }

    async fn load(env: &str) -> Result<Arc<Config>, AppError> {
        json_tracing::debug!("Loading config", env);

        let secret_name = env::var("AWS_SECRET_NAME").unwrap_or("on-call-support/secrets".to_string());
        let table_name_prefix = env::var("TABLE_NAME_PREFIX").unwrap_or("on-call-support-".to_string());
        let schedule_name_prefix = env::var("SCHEDULE_NAME_PREFIX").unwrap_or("on-call-support-".to_string());
        let admin_user_slack_ids = env::var("ADMIN_USER_SLACK_IDS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.trim().to_string())
            .collect();
        let aws_config = ::aws_config::load_defaults(BehaviorVersion::latest()).await;

        Ok(Arc::new(Config {
            env: env.to_string(),
            schedules_table_name: format!("{}schedules-{}", table_name_prefix, env),
            installations_table_name: format!("{}installations-{}", table_name_prefix, env),
            schedule_name_prefix: format!("{}{}_UpdateUserGroupSchedule_", schedule_name_prefix, env),
            admin_user_slack_ids,

            aws_config,
            secrets_cache: OnceCell::new(),
            secret_name,
        }))
    }

    pub async fn secrets(&self) -> Result<&Secrets, AppError> {
        json_tracing::debug!("Getting secrets", secret_name = &self.secret_name);
        let result = self
            .secrets_cache
            .get_or_try_init(|| async {
                json_tracing::info!("Loading secrets from Secrets Manager", secret_name = &self.secret_name);
                let secrets_client = SecretsClient::new(&self.aws_config);
                secrets_client.get_secret(&self.secret_name).await
            })
            .await;

        json_tracing::debug!("Got secrets", secret_name = &self.secret_name);
        result
    }

    pub async fn build_encryptor(&self) -> Result<Arc<dyn Encryptor + Send + Sync>, AppError> {
        let kms_key_id = env::var("KMS_KEY_ID").ok();
        let secret_id = env::var("AWS_SECRET_ID").ok();

        if let Some(kms_key_id) = kms_key_id {
            json_tracing::info!("Using AWS KMS encryption", kms_key_id);
            let kms_client = KmsClient::new(&self.aws_config);
            let encryptor = AWSKMSEncryptor::new(kms_client, kms_key_id).await?;
            Ok(Arc::new(encryptor))
        } else if let Some(secret_id) = secret_id {
            json_tracing::info!("Using XChaCha20 encryption with secret key", aws_secret_id = &secret_id);
            let secrets_client = SecretsClient::new(&self.aws_config);
            let encryption_key = secrets_client.get_secret_value(&secret_id).await?;

            let encryptor = XChaCha20Encryptor::from_key(&encryption_key)?;
            Ok(Arc::new(encryptor))
        } else {
            json_tracing::info!("Using XChaCha20 encryption");
            let secrets = self.secrets().await?;
            let encryptor = XChaCha20Encryptor::from_key(&secrets.encryption_key)?;
            Ok(Arc::new(encryptor))
        }
    }
}
