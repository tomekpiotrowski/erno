use rand::Rng;

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
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..62);
            CHARSET[idx] as char
        })
        .collect()
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
        // Generate multiple tokens and ensure they're different
        let token1 = generate_secure_token(64);
        let token2 = generate_secure_token(64);
        assert_ne!(token1, token2);
    }
}
