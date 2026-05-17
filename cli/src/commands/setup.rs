use std::io::{self, Write};

use crate::global_config::{GithubConfig, GlobalConfig};

pub async fn handle_setup() {
    let defaults = GlobalConfig::default();

    println!("This will configure your global erno settings at ~/.erno/config.toml.\n");

    let admin_url = prompt(
        &format!(
            "PostgreSQL admin connection URL [{}]",
            defaults.postgres.admin_url
        ),
        &defaults.postgres.admin_url,
    );

    print!("Verifying connection... ");
    io::stdout().flush().unwrap();

    match verify_postgres_connection(&admin_url).await {
        Ok(()) => println!("ok"),
        Err(e) => {
            println!("failed");
            eprintln!("  Could not connect: {e}");
            eprintln!("  Check that PostgreSQL is running and the credentials are correct.");
            std::process::exit(1);
        }
    }

    println!("\nGitHub personal access token (optional — enables `erno deploy` automation).");
    println!("Required scopes: repo, write:packages");
    println!("Create one at: https://github.com/settings/tokens/new");
    let github_token_input = prompt("GitHub token [skip]", "");

    let github = if github_token_input.is_empty() {
        None
    } else {
        print!("Verifying GitHub token... ");
        io::stdout().flush().unwrap();
        match verify_github_token(&github_token_input).await {
            Ok(login) => {
                println!("ok ({})", login);
                Some(GithubConfig { token: github_token_input })
            }
            Err(e) => {
                println!("failed");
                eprintln!("  Could not verify token: {e}");
                eprintln!("  Skipping GitHub configuration.");
                None
            }
        }
    };

    let config = GlobalConfig {
        postgres: crate::global_config::PostgresConfig { admin_url },
        github,
    };

    match config.save() {
        Ok(()) => {
            println!(
                "\n✅  Config saved to {}",
                GlobalConfig::path().unwrap().display()
            );
            println!("    Run `erno doctor` to verify your environment.");
        }
        Err(e) => {
            eprintln!("❌  Failed to write config: {e}");
            std::process::exit(1);
        }
    }
}

fn prompt(label: &str, default: &str) -> String {
    print!("{label}: ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("failed to read stdin");
    let trimmed = input.trim();

    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

async fn verify_github_token(token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "erno-cli")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let login = json["login"]
        .as_str()
        .ok_or("missing login field")?
        .to_string();
    Ok(login)
}

async fn verify_postgres_connection(url: &str) -> Result<(), tokio_postgres::Error> {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    // Quick privilege check: attempt to create and drop a probe database
    client
        .execute("CREATE DATABASE erno_setup_probe", &[])
        .await
        .ok(); // may already exist from a previous run
    client
        .execute("DROP DATABASE IF EXISTS erno_setup_probe", &[])
        .await?;
    Ok(())
}
