use base64::{Engine as _, engine::general_purpose};

use crate::errors::AppError;

pub fn encode_no_pad(bytes: &[u8]) -> String {
    general_purpose::STANDARD_NO_PAD.encode(bytes)
}

pub fn encode_with_pad(bytes: &[u8]) -> String {
    general_purpose::STANDARD.encode(bytes)
}

pub fn decode_no_pad(encoded: &[u8]) -> Result<Vec<u8>, AppError> {
    Ok(general_purpose::STANDARD_NO_PAD.decode::<&[u8]>(encoded)?)
}

#[cfg(test)]
mod tests {
    use crate::base64::{encode_no_pad, decode_no_pad};

    #[test]
    fn base64_encode_decode() {
        let original = "plain text string";
        let encoded = encode_no_pad(original.as_bytes());
        assert_eq!(encoded, "cGxhaW4gdGV4dCBzdHJpbmc");

        let decoded_bytes = decode_no_pad(encoded.as_bytes()).expect("failed to decode base64 encoded string");
        let decoded = String::from_utf8(decoded_bytes).expect("Failed to get string back from decoded bytes");

        assert_eq!(decoded, original);
    }
}
