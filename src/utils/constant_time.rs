//! Constant-time comparison utilities for security-critical operations.
//!
//! This module provides constant-time comparison functions to prevent timing attacks
//! when comparing secret values like HMAC signatures, passwords, or tokens.

use subtle::ConstantTimeEq;

/// Compares two byte slices in constant time.
///
/// This function takes the same amount of time regardless of where the
/// first difference between the slices occurs, preventing timing attacks.
///
/// # Arguments
///
/// * `a` - First byte slice to compare
/// * `b` - Second byte slice to compare
///
/// # Returns
///
/// `true` if slices are equal, `false` otherwise
///
/// # Examples
///
/// ```
/// use on_call_support::utils::constant_time::constant_time_compare;
///
/// let secret = b"my_secret_signature";
/// let provided = b"my_secret_signature";
///
/// assert!(constant_time_compare(secret, provided));
/// ```
///
/// # Security Notes
///
/// This function is critical for preventing timing attacks when comparing:
/// - HMAC signatures
/// - Password hashes
/// - API tokens
/// - Session tokens
/// - Any secret values
///
/// **DO NOT** use regular `==` comparison for secret values, as it will
/// short-circuit on the first byte difference, leaking timing information
/// that attackers can exploit.
pub fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    // First, check if lengths are equal
    // This is safe to do in non-constant time because:
    // 1. Length is not secret information (attacker knows signature length)
    // 2. Prevents buffer overread attacks
    if a.len() != b.len() {
        return false;
    }

    // Use the subtle crate's constant-time equality check
    // This always compares all bytes, regardless of matches
    a.ct_eq(b).into()
}

/// Compares two strings in constant time.
///
/// Convenience wrapper around `constant_time_compare` for string types.
///
/// # Arguments
///
/// * `a` - First string to compare
/// * `b` - Second string to compare
///
/// # Returns
///
/// `true` if strings are equal, `false` otherwise
///
/// # Examples
///
/// ```
/// use on_call_support::utils::constant_time::constant_time_compare_str;
///
/// let expected_signature = "v0=abc123def456";
/// let provided_signature = "v0=abc123def456";
///
/// assert!(constant_time_compare_str(expected_signature, provided_signature));
/// ```
pub fn constant_time_compare_str(a: &str, b: &str) -> bool {
    constant_time_compare(a.as_bytes(), b.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equal_slices() {
        let a = b"hello world";
        let b = b"hello world";
        assert!(constant_time_compare(a, b));
    }

    #[test]
    fn test_different_slices() {
        let a = b"hello world";
        let b = b"hello earth";
        assert!(!constant_time_compare(a, b));
    }

    #[test]
    fn test_different_lengths() {
        let a = b"hello";
        let b = b"hello world";
        assert!(!constant_time_compare(a, b));
    }

    #[test]
    fn test_first_byte_different() {
        let a = b"hello";
        let b = b"jello";
        assert!(!constant_time_compare(a, b));
    }

    #[test]
    fn test_last_byte_different() {
        let a = b"hello";
        let b = b"hella";
        assert!(!constant_time_compare(a, b));
    }

    #[test]
    fn test_middle_byte_different() {
        let a = b"hello";
        let b = b"hallo";
        assert!(!constant_time_compare(a, b));
    }

    #[test]
    fn test_equal_strings() {
        let a = "v0=abc123def456";
        let b = "v0=abc123def456";
        assert!(constant_time_compare_str(a, b));
    }

    #[test]
    fn test_different_strings() {
        let a = "v0=abc123def456";
        let b = "v0=abc123def999";
        assert!(!constant_time_compare_str(a, b));
    }

    #[test]
    fn test_empty_slices() {
        let a = b"";
        let b = b"";
        assert!(constant_time_compare(a, b));
    }

    #[test]
    fn test_hmac_signature_format() {
        // Simulate HMAC signature comparison
        let computed = "v0=1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let provided = "v0=1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        assert!(constant_time_compare_str(computed, provided));
    }

    #[test]
    fn test_hmac_signature_different() {
        let computed = "v0=1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let provided = "v0=1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdeg";
        // Only last character different
        assert!(!constant_time_compare_str(computed, provided));
    }
}
