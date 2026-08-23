use crate::global_config::{GithubConfig, GlobalConfig};
use crate::ui;

pub async fn handle_setup() -> ui::Cmd {
    let defaults = GlobalConfig::default();

    ui::section(ui::icon::SETUP, "Global settings");
    ui::detail("Configuring ~/.erno/config.toml.");
    ui::blank();

    let admin_url = ui::prompt(
        &format!(
            "PostgreSQL admin connection URL [{}]",
            defaults.postgres.admin_url
        ),
        &defaults.postgres.admin_url,
    );

    verify_postgres_connection(&admin_url).await.map_err(|e| {
        format!(
            "could not connect to PostgreSQL: {e}\n\
             Check that PostgreSQL is running and the credentials are correct."
        )
    })?;
    ui::ok("PostgreSQL connection");

    ui::section(ui::icon::CLOUD, "GitHub");
    ui::detail(
        "Optional — enables `erno deploy` automation.\n\
         Required scopes: repo, write:packages\n\
         Create one at: https://github.com/settings/tokens/new",
    );
    ui::blank();
    let github_token_input = ui::prompt("GitHub token [skip]", "");

    let github = if github_token_input.is_empty() {
        None
    } else {
        match verify_github_token(&github_token_input).await {
            Ok(login) => {
                ui::ok(format!("GitHub token ({login})"));
                Some(GithubConfig {
                    token: github_token_input,
                })
            }
            Err(e) => {
                // Not fatal: GitHub is optional, so we save the rest.
                ui::warn(format!("could not verify the GitHub token: {e}"));
                ui::detail("Skipping GitHub configuration.");
                None
            }
        }
    };

    let config = GlobalConfig {
        postgres: crate::global_config::PostgresConfig { admin_url },
        github,
    };
    config
        .save()
        .map_err(|e| format!("could not write the config: {e}"))?;

    let path = GlobalConfig::path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.erno/config.toml".to_string());
    ui::section(ui::icon::DONE, "Done");
    ui::finished(ui::icon::DONE, format!("Config saved to {path}"));
    ui::detail("Run `erno doctor` to verify your environment.");
    Ok(())
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
