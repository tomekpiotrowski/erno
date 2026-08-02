use std::io::{self, Write};
use std::path::Path;

use crate::admin::{client::AdminClient, tui};

/// Documented default for scaffolded development.toml admin hashes.
const DEV_DEFAULT_PASSWORD: &str = "admin";

pub async fn handle_admin(
    url: Option<String>,
    user: String,
    password: Option<String>,
    password_env: Option<String>,
) {
    let base_url = url.unwrap_or_else(detect_api_url);
    let password = resolve_password(&base_url, password, password_env);

    let client = match AdminClient::new(&base_url, &user, &password) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to create admin client: {e}");
            std::process::exit(1);
        }
    };

    // Probe auth before entering the TUI.
    match client.dashboard().await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Cannot reach admin API at {base_url}: {e}");
            eprintln!("  • Is the API running? (`erno dev` or `cargo run` in api/)");
            eprintln!("  • Is admin configured? (admin.password_hash in config)");
            if is_local_dev_url(&base_url) {
                eprintln!("  • Local default password is `{DEV_DEFAULT_PASSWORD}` (override with --password)");
            } else {
                eprintln!("  • Pass --password / --password-env / ERNO_ADMIN_PASSWORD");
            }
            std::process::exit(1);
        }
    }

    let plans = match client.plans().await {
        Ok(p) => p.plans,
        Err(_) => Vec::new(),
    };

    let handle = tokio::runtime::Handle::current();
    tokio::task::block_in_place(|| {
        if let Err(e) = tui::run(client, &handle, plans, base_url) {
            eprintln!("Admin TUI error: {e}");
            std::process::exit(1);
        }
    });
}

fn detect_api_url() -> String {
    // Prefer project development config when run from project root or api/.
    for candidate in ["api/config/development.toml", "config/development.toml"] {
        if let Some(url) = read_api_url_from_toml(Path::new(candidate)) {
            return url;
        }
    }
    "http://localhost:3000".to_string()
}

fn read_api_url_from_toml(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("api_url") {
            if let Some(val) = rest.trim().strip_prefix('=') {
                let val = val.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

fn resolve_password(
    base_url: &str,
    password: Option<String>,
    password_env: Option<String>,
) -> String {
    if let Some(p) = password {
        return p;
    }
    if let Some(var) = password_env {
        match std::env::var(&var) {
            Ok(p) if !p.is_empty() => return p,
            Ok(_) => {
                eprintln!("Environment variable {var} is empty.");
                std::process::exit(1);
            }
            Err(_) => {
                eprintln!("Environment variable {var} is not set.");
                std::process::exit(1);
            }
        }
    }
    if let Ok(p) = std::env::var("ERNO_ADMIN_PASSWORD") {
        if !p.is_empty() {
            return p;
        }
    }

    // Scaffolded development configs use password "admin" — skip the prompt
    // for local URLs so `erno admin` is frictionless during development.
    if is_local_dev_url(base_url) {
        return DEV_DEFAULT_PASSWORD.to_string();
    }

    eprint!("Admin password: ");
    let _ = io::stderr().flush();
    match rpassword::read_password() {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => {
            eprintln!("Password cannot be empty.");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to read password: {e}");
            std::process::exit(1);
        }
    }
}

/// True for loopback hosts (localhost / 127.0.0.1 / ::1), used to apply the
/// documented development admin password without prompting.
fn is_local_dev_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    matches!(
        parsed.host_str(),
        Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
    )
}
