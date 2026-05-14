use rand::Rng;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

/// Generate a cryptographically secure random token.
///
/// Creates a random alphanumeric string of the specified length suitable for
/// use as verification tokens, password reset tokens, or other security-sensitive
/// identifiers.
///
/// # Arguments
/// * `length` - The desired length of the token (typically 32 or 64 characters)
///
/// # Returns
/// A string containing random alphanumeric characters (A-Z, a-z, 0-9)
pub fn generate_secure_token(length: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..length)
        .map(|_| {
            let idx = OsRng.gen_range(0..62usize);
            CHARSET[idx] as char
        })
        .collect()
}

/// Returns the SHA-256 hex digest of a token string.
/// Store this hash in the database; send the raw token to users.
pub fn hash_token(token: &str) -> String {
    let hash = Sha256::digest(token.as_bytes());
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_secure_token_length() {
        let token = generate_secure_token(64);
        assert_eq!(token.len(), 64);
    }

    #[test]
    fn test_generate_secure_token_alphanumeric() {
        let token = generate_secure_token(64);
        assert!(token.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_secure_token_randomness() {
        let token1 = generate_secure_token(64);
        let token2 = generate_secure_token(64);
        assert_ne!(token1, token2);
    }

    #[test]
    fn test_hash_token_is_deterministic() {
        let token = "abc123";
        assert_eq!(hash_token(token), hash_token(token));
    }

    #[test]
    fn test_different_tokens_produce_different_hashes() {
        assert_ne!(hash_token("token_a"), hash_token("token_b"));
    }
}
