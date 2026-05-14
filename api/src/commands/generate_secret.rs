use base64::{engine::general_purpose, Engine as _};
use rand::RngCore;

/// Generates a cryptographically secure random secret suitable for JWT signing.
///
/// This function creates a 64-byte random secret using a cryptographically secure
/// random number generator and encodes it as base64 for easy storage in configuration files.
///
/// The generated secret should be kept confidential and stored securely in the application
/// configuration.
pub fn handle_generate_secret_command() {
    let mut secret = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut secret);
    let encoded = general_purpose::STANDARD.encode(secret);

    println!("🔐 Generated JWT Secret:");
    println!();
    println!("auth:");
    println!("  secret: \"{}\"", encoded);
    println!();
    println!("Add this to your config/{{environment}}.yaml file.");
}
