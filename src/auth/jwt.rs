use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Config;

/// JWT claims structure containing user information and token metadata.
///
/// This structure defines the payload of the JWT token. The `sub` (subject) field
/// contains the user ID, while `exp` (expiration) and `iat` (issued at) provide
/// standard JWT timing claims.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject - the user ID
    pub sub: String,
    /// Token version — must match `users.token_version`. Incremented on logout
    /// and password change to invalidate outstanding tokens.
    pub ver: i32,
    /// Expiration time (Unix timestamp)
    pub exp: usize,
    /// Issued at (Unix timestamp)
    pub iat: usize,
}

/// Generate a JWT token for the specified user.
///
/// Creates a signed JWT token with the user's ID as the subject and expiration
/// set according to the configuration. The token is signed using the HS256 algorithm
/// with the secret from the application configuration.
///
/// # Arguments
/// * `config` - Application configuration containing JWT secret and expiration settings
/// * `user_id` - UUID of the user to create the token for
///
/// # Returns
/// A signed JWT token string, or an error if token generation fails
///
/// # Errors
/// Returns `jsonwebtoken::errors::Error` if token encoding fails
pub fn generate_token<ExtraConfig>(
    config: &Config<ExtraConfig>,
    user_id: Uuid,
    token_version: i32,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now().timestamp() as usize;
    let exp = now + (config.jwt.expiration_days * 86400) as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        ver: token_version,
        exp,
        iat: now,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt.secret.as_bytes()),
    )
}

/// Verify and decode a JWT token.
///
/// Validates the token signature and expiration, then returns the decoded claims.
///
/// # Arguments
/// * `config` - Application configuration containing JWT secret
/// * `token` - The JWT token string to verify
///
/// # Returns
/// The decoded claims if the token is valid, or an error if verification fails
///
/// # Errors
/// Returns `jsonwebtoken::errors::Error` if token is invalid, expired, or malformed
/// Minimum acceptable byte length for an HS256 JWT secret (256 bits).
const MIN_JWT_SECRET_LEN: usize = 32;

/// Validate that the JWT secret meets minimum length requirements.
///
/// Returns `Err` with a descriptive message when the secret is too short.
/// Call this at startup so misconfiguration is caught before serving traffic.
pub fn validate_jwt_secret<ExtraConfig>(config: &Config<ExtraConfig>) -> Result<(), String> {
    if config.jwt.secret.len() < MIN_JWT_SECRET_LEN {
        return Err(format!(
            "JWT secret is too short ({} bytes). Minimum is {} bytes (256 bits).",
            config.jwt.secret.len(),
            MIN_JWT_SECRET_LEN,
        ));
    }
    Ok(())
}

pub fn verify_token<ExtraConfig>(
    config: &Config<ExtraConfig>,
    token: &str,
) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt.secret.as_bytes()),
        &Validation::default(),
    )?;

    Ok(token_data.claims)
}
