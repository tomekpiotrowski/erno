use std::io::{self, Write};

use crate::global_config::GlobalConfig;

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

    let config = GlobalConfig {
        postgres: crate::global_config::PostgresConfig { admin_url },
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
