use std::process::Command;

use crate::global_config::GlobalConfig;

const MIN_RUST_MINOR: u32 = 88;

enum Status {
    Pass,
    Warn,
    Fail,
}

struct CheckResult {
    status: Status,
    label: String,
    detail: Option<String>,
    hint: Option<String>,
    required: bool,
}

impl CheckResult {
    fn pass(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: Status::Pass,
            label: label.into(),
            detail: Some(detail.into()),
            hint: None,
            required: true,
        }
    }

    fn warn(label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            status: Status::Warn,
            label: label.into(),
            detail: None,
            hint: Some(hint.into()),
            required: false,
        }
    }

    fn fail(label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            status: Status::Fail,
            label: label.into(),
            detail: None,
            hint: Some(hint.into()),
            required: true,
        }
    }
}

pub async fn handle_doctor() {
    println!();
    let results = run_checks().await;

    let mut any_failed = false;
    for r in &results {
        match r.status {
            Status::Pass => {
                let detail = r.detail.as_deref().unwrap_or("");
                println!(
                    "  ✅  {}{}",
                    r.label,
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!(" ({detail})")
                    }
                );
            }
            Status::Warn => {
                println!("  ⚠️   {}", r.label);
                if let Some(h) = &r.hint {
                    println!("      {h}");
                }
            }
            Status::Fail => {
                println!("  ❌  {}", r.label);
                if let Some(h) = &r.hint {
                    println!("      {h}");
                }
                if r.required {
                    any_failed = true;
                }
            }
        }
    }

    println!();
    if any_failed {
        eprintln!("Some required checks failed. Fix the issues above and run `erno doctor` again.");
        std::process::exit(1);
    } else {
        println!("All required checks passed.");
    }
}

async fn run_checks() -> Vec<CheckResult> {
    vec![
        check_rust(),
        check_node(),
        check_npm(),
        check_angular_cli(),
        check_psql(),
        check_pg_isready(),
        check_global_config(),
        check_postgres_admin().await,
        check_sea_orm_cli(),
    ]
}

fn check_rust() -> CheckResult {
    let out = run_cmd("rustc", &["--version"]);
    match out {
        None => CheckResult::fail("Rust", "Install from https://rustup.rs"),
        Some(v) => {
            // "rustc 1.88.0 (xxxxxxx YYYY-MM-DD)"
            if let Some(ver) = parse_version_after(&v, "rustc ") {
                let minor = ver
                    .split('.')
                    .nth(1)
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                if minor >= MIN_RUST_MINOR {
                    CheckResult::pass("Rust", ver)
                } else {
                    CheckResult::fail(
                        "Rust",
                        format!("Version {ver} is too old — 1.{MIN_RUST_MINOR}+ required. Run: rustup update"),
                    )
                }
            } else {
                CheckResult::pass("Rust", v.trim().to_string())
            }
        }
    }
}

fn check_node() -> CheckResult {
    match run_cmd("node", &["--version"]) {
        None => CheckResult::fail("Node.js", "Install from https://nodejs.org"),
        Some(v) => CheckResult::pass("Node.js", v.trim().to_string()),
    }
}

fn check_npm() -> CheckResult {
    match run_cmd("npm", &["--version"]) {
        None => CheckResult::fail("npm", "Install Node.js (includes npm): https://nodejs.org"),
        Some(v) => CheckResult::pass("npm", v.trim().to_string()),
    }
}

fn check_angular_cli() -> CheckResult {
    // `ng version` outputs to stdout but exits non-zero in some environments;
    // capture both stdout and stderr and accept either.
    let output =
        crate::ng::find_ng_binary().and_then(|ng| Command::new(ng).arg("version").output().ok());

    match output {
        None => CheckResult::fail("Angular CLI", "Install with: npm install -g @angular/cli"),
        Some(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let ver = text
                .lines()
                .find(|l| l.contains("Angular CLI"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "found".to_string());
            CheckResult::pass("Angular CLI", ver)
        }
    }
}

fn check_psql() -> CheckResult {
    match run_cmd("psql", &["--version"]) {
        None => CheckResult::fail(
            "PostgreSQL client (psql)",
            "Install PostgreSQL: https://www.postgresql.org/download/",
        ),
        Some(v) => {
            // "psql (PostgreSQL) 16.3" → "16.3"
            let ver = parse_version_after(v.trim(), ") ").unwrap_or(v.trim());
            CheckResult::pass("PostgreSQL client", ver.to_string())
        }
    }
}

fn check_pg_isready() -> CheckResult {
    let output = Command::new("pg_isready").output();
    match output {
        Err(_) => CheckResult::fail(
            "PostgreSQL server",
            "pg_isready not found — install PostgreSQL client tools",
        ),
        Ok(o) => {
            if o.status.success() {
                CheckResult::pass("PostgreSQL server", "running")
            } else {
                CheckResult::fail(
                    "PostgreSQL server not running",
                    "Start PostgreSQL — e.g.: sudo service postgresql start",
                )
            }
        }
    }
}

fn check_global_config() -> CheckResult {
    if GlobalConfig::exists() {
        CheckResult::pass("~/.erno/config.toml", "found")
    } else {
        CheckResult::fail("~/.erno/config.toml not found", "Run: erno setup")
    }
}

async fn check_postgres_admin() -> CheckResult {
    let config = match GlobalConfig::load() {
        Ok(c) => c,
        Err(_) => {
            return CheckResult::fail(
                "PostgreSQL admin access",
                "Config missing — run: erno setup",
            )
        }
    };

    let url = &config.postgres.admin_url;
    match tokio_postgres::connect(url, tokio_postgres::NoTls).await {
        Err(e) => CheckResult::fail(
            "PostgreSQL admin access",
            format!("Could not connect ({e}) — run: erno setup"),
        ),
        Ok((client, connection)) => {
            tokio::spawn(async move {
                let _ = connection.await;
            });
            match client
                .query_one(
                    "SELECT rolcreatedb OR rolsuper, rolcreaterole OR rolsuper \
                     FROM pg_roles WHERE rolname = current_user",
                    &[],
                )
                .await
            {
                Ok(row) => {
                    let can_createdb: bool = row.get(0);
                    let can_createrole: bool = row.get(1);
                    let user = parse_pg_user(url);
                    match (can_createdb, can_createrole) {
                        (true, true) => CheckResult::pass(
                            "PostgreSQL admin access",
                            "can create databases and roles",
                        ),
                        (false, _) => CheckResult::fail(
                            "PostgreSQL admin access",
                            format!("User '{user}' lacks CREATEDB. Fix with: ALTER USER {user} CREATEDB;"),
                        ),
                        (true, false) => CheckResult::fail(
                            "PostgreSQL admin access",
                            format!("User '{user}' lacks CREATEROLE (needed by `erno new`). Fix with: ALTER USER {user} CREATEROLE;"),
                        ),
                    }
                }
                Err(e) => {
                    let msg = e
                        .as_db_error()
                        .map(|d| d.message().to_string())
                        .unwrap_or_else(|| e.to_string());
                    CheckResult::fail(
                        "PostgreSQL admin access",
                        format!("Connected but could not check privileges: {msg}"),
                    )
                }
            }
        }
    }
}

fn parse_pg_user(url: &str) -> &str {
    url.split('@')
        .next()
        .and_then(|s| s.split("//").nth(1))
        .and_then(|s| s.split(':').next())
        .unwrap_or("?")
}

fn check_sea_orm_cli() -> CheckResult {
    match run_cmd("sea-orm-cli", &["--version"]) {
        None => CheckResult::warn(
            "sea-orm-cli not found",
            "Install with: cargo install sea-orm-cli",
        ),
        Some(v) => CheckResult::pass("sea-orm-cli", v.trim().to_string()),
    }
}

fn run_cmd(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn parse_version_after<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.find(prefix).map(|i| {
        s[i + prefix.len()..]
            .split_whitespace()
            .next()
            .unwrap_or("")
    })
}
