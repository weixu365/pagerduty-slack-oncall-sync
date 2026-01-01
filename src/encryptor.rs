
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    XChaCha20Poly1305, XNonce
};
use serde_derive::{Serialize, Deserialize};
use crate::base64;
use crate::errors::AppError;

#[derive(Serialize, Deserialize, Debug)]
pub struct EncryptedData {
    pub nonce: String,
    pub data: String,
}

#[derive(Clone)]
pub struct Encryptor {
    cipher: XChaCha20Poly1305,
}

impl Encryptor {
    pub fn from_key(key: &str) -> Result<Encryptor, AppError> {
        let key_bytes = key.as_bytes();

        if key_bytes.len() != 32 {
            return Err(AppError::InvalidKeyLength(key_bytes.len()));
        }

        let cipher = XChaCha20Poly1305::new(key_bytes.into());
    
        Ok(Encryptor { cipher })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<EncryptedData, AppError> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng); // 192-bits; unique per message
        let ciphertext = self.cipher.encrypt(&nonce, plaintext.as_bytes())?;
    
        Ok(EncryptedData{
            nonce: base64::encode_no_pad(nonce.as_slice()),
            data: base64::encode_no_pad(&ciphertext),
        })
    }
    
    pub fn decrypt(&self, encrypted_data: &EncryptedData) -> Result<String, AppError> {
        let nonce_bytes = base64::decode_no_pad(encrypted_data.nonce.as_ref())?;
        let encrypted = base64::decode_no_pad(encrypted_data.data.as_ref())?;

        let nonce = XNonce::from_slice(nonce_bytes.as_slice());
        let plaintext = self.cipher.decrypt(&nonce, encrypted.as_slice())?;
        let decrypted = String::from_utf8(plaintext)?;

        Ok(decrypted)
    } 
}

#[cfg(test)]
mod tests {
    use crate::encryptor::{Encryptor, EncryptedData};

    #[test]
    fn encrypt_decrypt_string() -> Result<(), Box<dyn std::error::Error>> {
        let key_plaintext = "plain text key which should be s";
        
        let encryptor = Encryptor::from_key(&key_plaintext)?;
        
        let original = "plain text string";
        let encrypted = encryptor.encrypt(original)?;

        let encrypted_json = serde_json::to_string(&encrypted)?;

        let deserialized_from_json: EncryptedData = serde_json::from_str(&encrypted_json)?;
        let decrypted = encryptor.decrypt(&deserialized_from_json)?;
        
        assert_eq!(decrypted, original);
        Ok(())
    }
}
