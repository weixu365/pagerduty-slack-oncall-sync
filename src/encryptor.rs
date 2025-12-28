
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
    pub fn new(key: &str) -> Encryptor {
        let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    
        Encryptor {
            cipher,
        }
    }

    pub fn with(key_base64: &str) -> Encryptor {
        let key = base64::decode_no_pad(key_base64.as_ref()).expect("Failed to decode key, expecting base64 encoded");
        let cipher = XChaCha20Poly1305::new(key.as_slice().into());
    
        Encryptor {
            cipher,
        }
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
    
        Ok(String::from_utf8(plaintext).expect("Invalid UTF-8 sequence"))
    }    
}

#[cfg(test)]
mod tests {
    use crate::encryptor::{Encryptor, EncryptedData};

    #[test]
    fn encrypt_decrypt_string() {
        let key_plaintext = "plain text key which should be s";
        
        let encryptor = Encryptor::new(&key_plaintext);
        
        let original = "plain text string";
        let encrypted = encryptor.encrypt(original).expect("Failed to encrypt text");

        let encrypted_json = serde_json::to_string(&encrypted).unwrap();
        println!("encrypted: {:?}", encrypted_json);

        let deserialized_from_json: EncryptedData = serde_json::from_str(&encrypted_json).expect("couldn't parse json");
        let decrypted = encryptor.decrypt(&deserialized_from_json).expect("failed to decrypt encrypted data");
        
        assert_eq!(decrypted, original);
    }
}
