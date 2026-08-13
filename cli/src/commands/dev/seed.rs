use std::path::Path;
use std::time::Duration;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use reqwest::Client;

use super::ports::parse_table_string;
use super::{DIM, GREEN, RESET};

pub const DEMO_EMAIL: &str = "dev@example.com";
pub const DEMO_PASSWORD: &str = "password";

pub async fn maybe_seed(root: &Path, api_url: &str, force: bool) {
    if !wait_for_api(api_url).await {
        if force {
            eprintln!("Could not reach {api_url} to seed a demo user.");
        }
        return;
    }

    let toml = match std::fs::read_to_string(root.join("api/config/development.toml")) {
        Ok(s) => s,
        Err(_) => {
            if force {
                eprintln!("Cannot read api/config/development.toml — skip seed.");
            }
            return;
        }
    };
    let Some(db_url) = parse_table_string(&toml, "database", "url") else {
        if force {
            eprintln!("No [database].url in development.toml — skip seed.");
        }
        return;
    };

    match seed_demo_user(&db_url, force).await {
        Ok(SeedResult::Created) => {
            println!("{GREEN}Seeded demo user{RESET}  {DEMO_EMAIL} / {DEMO_PASSWORD}");
        }
        Ok(SeedResult::AlreadyPresent) if force => {
            println!("{DIM}Demo user {DEMO_EMAIL} already exists{RESET}");
        }
        Ok(SeedResult::SkippedNotEmpty) => {}
        Ok(SeedResult::AlreadyPresent) => {}
        Err(e) => eprintln!("Could not seed demo user: {e}"),
    }
}

enum SeedResult {
    Created,
    AlreadyPresent,
    SkippedNotEmpty,
}

async fn seed_demo_user(db_url: &str, force: bool) -> Result<SeedResult, String> {
    let (client, connection) = tokio_postgres::connect(db_url, tokio_postgres::NoTls)
        .await
        .map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let exists = client
        .query_opt(
            "SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'users'",
            &[],
        )
        .await
        .map_err(|e| e.to_string())?;
    if exists.is_none() {
        return Err("users table is not there yet (migrations still running?)".into());
    }

    let count: i64 = client
        .query_one("SELECT COUNT(*) FROM users", &[])
        .await
        .map_err(|e| e.to_string())?
        .get(0);
    if count > 0 && !force {
        return Ok(SeedResult::SkippedNotEmpty);
    }

    let existing = client
        .query_opt("SELECT 1 FROM users WHERE email = $1", &[&DEMO_EMAIL])
        .await
        .map_err(|e| e.to_string())?;
    if existing.is_some() {
        return Ok(SeedResult::AlreadyPresent);
    }

    let hash = hash_password(DEMO_PASSWORD)?;
    client
        .execute(
            "INSERT INTO users (email, password_hash, email_verified_at) \
             VALUES ($1, $2, NOW())",
            &[&DEMO_EMAIL, &hash],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(SeedResult::Created)
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

async fn wait_for_api(api_url: &str) -> bool {
    let client = match Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let url = format!("{}/readiness", api_url.trim_end_matches('/'));
    for _ in 0..600 {
        if let Ok(res) = client.get(&url).send().await {
            if res.status().as_u16() < 500 {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_verifiable() {
        use argon2::{
            password_hash::{PasswordHash, PasswordVerifier},
            Argon2,
        };
        let hash = hash_password("password").unwrap();
        let parsed = PasswordHash::new(&hash).unwrap();
        assert!(Argon2::default()
            .verify_password(b"password", &parsed)
            .is_ok());
    }
}
