use crate::errors::AppError;
use crate::utils::base64;
use async_trait::async_trait;
use aws_esdk::client as esdk_client;
use aws_esdk::material_providers::client as mpl_client;
use aws_esdk::material_providers::types::material_providers_config::MaterialProvidersConfig;
use aws_esdk::types::aws_encryption_sdk_config::AwsEncryptionSdkConfig;
use aws_sdk_kms::Client as KmsClient;
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EncryptedData {
    pub nonce: String,
    pub data: String,
}

#[async_trait]
pub trait Encryptor {
    async fn encrypt(&self, plaintext: &str) -> Result<String, AppError>;
    async fn decrypt(&self, ciphertext: &str) -> Result<String, AppError>;
}

#[derive(Clone)]
pub struct XChaCha20Encryptor {
    cipher: XChaCha20Poly1305,
}

impl XChaCha20Encryptor {
    pub fn from_key(key: &str) -> Result<XChaCha20Encryptor, AppError> {
        let key_bytes = key.as_bytes();

        if key_bytes.len() != 32 {
            return Err(AppError::InvalidKeyLength(key_bytes.len()));
        }

        let cipher = XChaCha20Poly1305::new(key_bytes.into());

        Ok(XChaCha20Encryptor { cipher })
    }
}

#[async_trait]
impl Encryptor for XChaCha20Encryptor {
    async fn encrypt(&self, plaintext: &str) -> Result<String, AppError> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng); // 192-bits; unique per message
        let ciphertext = self.cipher.encrypt(&nonce, plaintext.as_bytes())?;

        let encrypted_data = EncryptedData {
            nonce: base64::encode_no_pad(nonce.as_slice()),
            data: base64::encode_no_pad(&ciphertext),
        };

        let json = serde_json::to_string(&encrypted_data)?;
        Ok(json)
    }

    async fn decrypt(&self, ciphertext: &str) -> Result<String, AppError> {
        let encrypted_data: EncryptedData = serde_json::from_str(ciphertext)?;

        let nonce_bytes = base64::decode_no_pad(encrypted_data.nonce.as_ref())?;
        let encrypted = base64::decode_no_pad(encrypted_data.data.as_ref())?;

        let nonce = XNonce::from_slice(nonce_bytes.as_slice());
        let plaintext = self.cipher.decrypt(&nonce, encrypted.as_slice())?;
        let decrypted = String::from_utf8(plaintext)?;

        Ok(decrypted)
    }
}

#[derive(Clone)]
pub struct AWSKMSEncryptor {
    encryption_client: esdk_client::Client,
    keyring: aws_esdk::material_providers::types::keyring::KeyringRef,
}

impl AWSKMSEncryptor {
    pub async fn new(kms_client: KmsClient, key_id: String) -> Result<AWSKMSEncryptor, AppError> {
        let encryption_config = AwsEncryptionSdkConfig::builder()
            .build()
            .map_err(|e| AppError::EncryptionError(format!("Failed to build ESDK config: {}", e)))?;
        let encryption_client = esdk_client::Client::from_conf(encryption_config)
            .map_err(|e| AppError::EncryptionError(format!("Failed to create ESDK client: {}", e)))?;

        let material_providers_config = MaterialProvidersConfig::builder()
            .build()
            .map_err(|e| AppError::EncryptionError(format!("Failed to build MPL config: {}", e)))?;
        let material_providers_client = mpl_client::Client::from_conf(material_providers_config)
            .map_err(|e| AppError::EncryptionError(format!("Failed to create MPL client: {}", e)))?;

        let keyring = material_providers_client
            .create_aws_kms_keyring()
            .kms_key_id(key_id)
            .kms_client(kms_client)
            .send()
            .await
            .map_err(|e| AppError::EncryptionError(format!("Failed to create KMS keyring: {}", e)))?;

        Ok(AWSKMSEncryptor {
            encryption_client,
            keyring,
        })
    }
}

#[async_trait]
impl Encryptor for AWSKMSEncryptor {
    async fn encrypt(&self, plaintext: &str) -> Result<String, AppError> {
        let plaintext_bytes = plaintext.as_bytes();

        let encryption_response = self
            .encryption_client
            .encrypt()
            .plaintext(plaintext_bytes)
            .keyring(self.keyring.clone())
            .send()
            .await
            .map_err(|e| AppError::EncryptionError(format!("AWS KMS encryption failed: {}", e)))?;

        let ciphertext = encryption_response
            .ciphertext
            .ok_or_else(|| AppError::EncryptionError("No ciphertext in encryption response".to_string()))?;

        Ok(base64::encode_no_pad(ciphertext.as_ref()))
    }

    async fn decrypt(&self, ciphertext: &str) -> Result<String, AppError> {
        let encrypted_bytes = base64::decode_no_pad(ciphertext.as_bytes())?;

        let decryption_response = self
            .encryption_client
            .decrypt()
            .ciphertext(encrypted_bytes.as_slice())
            .keyring(self.keyring.clone())
            .send()
            .await
            .map_err(|e| AppError::EncryptionError(format!("AWS KMS decryption failed: {}", e)))?;

        let plaintext = decryption_response
            .plaintext
            .ok_or_else(|| AppError::EncryptionError("No plaintext in decryption response".to_string()))?;

        let decrypted = String::from_utf8(plaintext.as_ref().to_vec())?;

        Ok(decrypted)
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use crate::encryptor::{AWSKMSEncryptor, Encryptor, XChaCha20Encryptor};

    #[tokio::test]
    async fn encrypt_decrypt_string() -> Result<(), Box<dyn std::error::Error>> {
        let key_plaintext = "plain text key which should be s";

        let encryptor = XChaCha20Encryptor::from_key(&key_plaintext)?;

        let original = "plain text string";
        let encrypted = encryptor.encrypt(original).await?;

        let decrypted = encryptor.decrypt(&encrypted).await?;

        assert_eq!(decrypted, original);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn encrypt_decrypt_string_with_aws_kms() -> Result<(), Box<dyn std::error::Error>> {
        let kms_key_id = match env::var("KMS_KEY_ID") {
            Ok(key) => key,
            Err(_) => {
                println!("SKIPPED: KMS_KEY_ID environment variable not set");
                return Ok(());
            }
        };

        let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let kms_client = aws_sdk_kms::Client::new(&sdk_config);

        let encryptor = AWSKMSEncryptor::new(kms_client, kms_key_id.to_string()).await?;

        let original = "plain text string for AWS KMS encryption test";

        let encrypted = encryptor.encrypt(original).await?;

        assert!(!encrypted.is_empty(), "Encrypted data should not be empty");

        let decryptor_kms_client = aws_sdk_kms::Client::new(&sdk_config);
        let decryptor = AWSKMSEncryptor::new(decryptor_kms_client, kms_key_id.to_string()).await?;

        let decrypted = decryptor.decrypt(&encrypted).await?;

        assert_eq!(decrypted, original);

        Ok(())
    }
}
