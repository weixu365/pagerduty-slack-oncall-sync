
use aws_config::SdkConfig;
use aws_sdk_secretsmanager::Client;
use serde_derive::{Serialize, Deserialize};

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
        SecretsClient{ client: Client::new(&config) }
    }
   
    pub async fn get_secret(&self, name: &str) -> Result<Secrets, AppError> {
        println!("Querying secret value for {}", name);

        let result = self.client
            .get_secret_value()
            .secret_id(name)
            .send()
            .await?;

        let secrets_value = result.secret_string().expect(format!("Couldn't get secret value for {}", name).as_str());
        let secrets: Secrets = serde_json::from_str(&secrets_value).expect("couldn't parse json");
        Ok(secrets)
    }
}

#[cfg(test)]
mod tests {
    use aws_config::BehaviorVersion;

    use crate::{secrets::SecretsClient, errors::AppError};

    #[tokio::test]
    async fn encrypt_decrypt_string() -> Result<(), AppError>{
        let config = ::aws_config::load_defaults(BehaviorVersion::latest()).await;
        let client = SecretsClient::new(&config);
        let encryption_key = client.get_secret("on-call-support/secrets").await?;
        
        println!("{:?}", encryption_key);
        assert_eq!(encryption_key.encryption_key.len(), 32);
        Ok(())
    }
}
