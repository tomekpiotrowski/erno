use argon2::{
    password_hash::{
        rand_core::OsRng,
        Error::{self, Password},
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2,
};

/// Generates a cryptographically secure salt and hashes the password using Argon2
pub fn hash_password(password: &str) -> Result<(String, String), Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string();

    Ok((salt.to_string(), password_hash))
}

/// Verifies a password against a stored hash and salt
pub fn verify_password(password: &str, _salt: &str, hash: &str) -> Result<bool, Error> {
    let argon2 = Argon2::default();

    // Reconstruct the full hash string that includes the salt
    let parsed_hash = PasswordHash::new(hash)?;

    match argon2.verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(Password) => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hashing_and_verification() {
        let password = "test_password_123";

        // Hash the password
        let (salt, hash) = hash_password(password).expect("Failed to hash password");

        // Verify the correct password
        assert!(verify_password(password, &salt, &hash).expect("Failed to verify password"));

        // Verify an incorrect password
        assert!(
            !verify_password("wrong_password", &salt, &hash).expect("Failed to verify password")
        );
    }

    #[test]
    fn test_different_salts_produce_different_hashes() {
        let password = "same_password";

        let (salt1, hash1) = hash_password(password).expect("Failed to hash password");
        let (salt2, hash2) = hash_password(password).expect("Failed to hash password");

        // Different salts should produce different hashes
        assert_ne!(salt1, salt2);
        assert_ne!(hash1, hash2);

        // But both should verify correctly
        assert!(verify_password(password, &salt1, &hash1).expect("Failed to verify password"));
        assert!(verify_password(password, &salt2, &hash2).expect("Failed to verify password"));
    }
}
