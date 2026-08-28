use std::process::Command;

use crate::global_config::GlobalConfig;
use crate::ui::{self, Row};

const MIN_RUST_MINOR: u32 = 88;

/// A check result is a [`Row`] plus whether its failure is fatal.
struct CheckResult {
    row: Row,
    required: bool,
}

impl CheckResult {
    fn pass(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            row: Row::ok(label, detail),
            required: true,
        }
    }

    /// `label` names the thing checked, `detail` says what is wrong with it,
    /// and `hint` says what to do. Keeping the label a bare noun is what lets
    /// every row — passing or not — share one label column.
    fn warn(label: impl Into<String>, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            row: Row {
                detail: Some(detail.into()),
                ..Row::warn(label, hint)
            },
            required: false,
        }
    }

    fn fail(label: impl Into<String>, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            row: Row {
                detail: Some(detail.into()),
                ..Row::fail(label, hint)
            },
            required: true,
        }
    }

    /// Only a *required* failure sinks the run — a warning never does.
    fn is_blocking(&self) -> bool {
        self.required && self.row.level == ui::Level::Fail
    }
}

pub async fn handle_doctor() -> ui::Cmd {
    let results = run_checks().await;

    ui::section(ui::icon::DOCTOR, "Environment");
    ui::blank();
    let rows: Vec<Row> = results.iter().map(|r| r.row.clone()).collect();
    ui::print_rows(&rows);

    let blocking = results.iter().filter(|r| r.is_blocking()).count();
    if blocking > 0 {
        ui::emit(ui::Stream::Err, "");
        let plural = if blocking == 1 { "check" } else { "checks" };
        return Err(ui::Failure::Message(format!(
            "{blocking} required {plural} failed\n\
             Fix the issues above and run `erno doctor` again."
        )));
    }
    // `doctor`'s result is its exit code, which is what makes `erno doctor -q`
    // usable as a check in a script: a healthy environment says nothing at all.
    // Every other command's result is a line, so `ui::finished` survives
    // `--quiet` and this is the one caller that opts out.
    if !ui::quiet() {
        ui::blank();
        ui::finished(ui::icon::DONE, "Everything checks out.");
    }
    Ok(())
}

async fn run_checks() -> Vec<CheckResult> {
    let mut results = vec![
        check_rust(),
        check_node(),
        check_npm(),
        check_angular_cli(),
        check_ionic_cli(),
        check_psql(),
        check_pg_isready(),
        check_global_config(),
        check_postgres_admin().await,
        check_sea_orm_cli(),
        check_prometheus(),
        check_tempo(),
        check_loki(),
    ];
    if deploy_dir_present() || std::path::Path::new("chart").is_dir() {
        results.push(check_kubectl());
        results.push(check_sops());
        results.push(check_age());
    }
    if let Some(row) = check_deploy_layout() {
        results.push(row);
    }
    results
}

fn check_rust() -> CheckResult {
    let out = run_cmd("rustc", &["--version"]);
    match out {
        None => CheckResult::fail("Rust", "not found", "Install from https://rustup.rs"),
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
                        format!("{ver} is too old — 1.{MIN_RUST_MINOR}+ required"),
                        "Run: rustup update",
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
        None => CheckResult::fail("Node.js", "not found", "Install from https://nodejs.org"),
        Some(v) => CheckResult::pass("Node.js", v.trim().to_string()),
    }
}

fn check_npm() -> CheckResult {
    match run_cmd("npm", &["--version"]) {
        None => CheckResult::fail(
            "npm",
            "not found",
            "Install Node.js (includes npm): https://nodejs.org",
        ),
        Some(v) => CheckResult::pass("npm", v.trim().to_string()),
    }
}

fn check_angular_cli() -> CheckResult {
    // `ng version` outputs to stdout but exits non-zero in some environments;
    // capture both stdout and stderr and accept either.
    let output =
        crate::ng::find_ng_binary().and_then(|ng| Command::new(ng).arg("version").output().ok());

    match output {
        None => CheckResult::fail(
            "Angular CLI",
            "not found",
            "Install with: npm install -g @angular/cli",
        ),
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

fn check_ionic_cli() -> CheckResult {
    match crate::ng::find_ionic_binary()
        .and_then(|ionic| Command::new(ionic).arg("--version").output().ok())
    {
        None => CheckResult::fail(
            "Ionic CLI",
            "not found",
            "Install with: npm install -g @ionic/cli",
        ),
        Some(out) => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            CheckResult::pass(
                "Ionic CLI",
                if ver.is_empty() {
                    "found".to_string()
                } else {
                    ver
                },
            )
        }
    }
}

fn check_psql() -> CheckResult {
    match run_cmd("psql", &["--version"]) {
        None => CheckResult::fail(
            "PostgreSQL client",
            "psql not found",
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
            "pg_isready not found",
            "Install the PostgreSQL client tools.",
        ),
        Ok(o) => {
            if o.status.success() {
                CheckResult::pass("PostgreSQL server", "running")
            } else {
                CheckResult::fail(
                    "PostgreSQL server",
                    "not running",
                    "Start it — e.g.: sudo service postgresql start",
                )
            }
        }
    }
}

fn check_global_config() -> CheckResult {
    if GlobalConfig::exists() {
        CheckResult::pass("~/.erno/config.toml", "found")
    } else {
        CheckResult::fail("~/.erno/config.toml", "not found", "Run: erno setup")
    }
}

async fn check_postgres_admin() -> CheckResult {
    let config = match GlobalConfig::load() {
        Ok(c) => c,
        Err(_) => {
            return CheckResult::fail(
                "PostgreSQL admin access",
                "config missing",
                "Run: erno setup",
            )
        }
    };

    let url = &config.postgres.admin_url;
    match tokio_postgres::connect(url, tokio_postgres::NoTls).await {
        Err(e) => CheckResult::fail(
            "PostgreSQL admin access",
            format!("could not connect ({e})"),
            "Run: erno setup",
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
                            format!("user '{user}' lacks CREATEDB"),
                            format!("Fix with: ALTER USER {user} CREATEDB;"),
                        ),
                        (true, false) => CheckResult::fail(
                            "PostgreSQL admin access",
                            format!("user '{user}' lacks CREATEROLE (needed by `erno new`)"),
                            format!("Fix with: ALTER USER {user} CREATEROLE;"),
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
                        "connected, but could not check privileges",
                        msg,
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
            "sea-orm-cli",
            "not found",
            "Install with: cargo install sea-orm-cli",
        ),
        Some(v) => CheckResult::pass("sea-orm-cli", v.trim().to_string()),
    }
}

fn check_kubectl() -> CheckResult {
    match run_cmd("kubectl", &["version", "--client", "--output=yaml"])
        .or_else(|| run_cmd("kubectl", &["version", "--client"]))
    {
        None => {
            if deploy_dir_present() {
                CheckResult::fail(
                    "kubectl",
                    "not found",
                    "Install kubectl 1.26+ — `erno deploy install` applies with server-side apply.",
                )
            } else {
                CheckResult::warn(
                    "kubectl",
                    "not found",
                    "Needed for `erno deploy`. Install kubectl 1.26+.",
                )
            }
        }
        Some(v) => CheckResult::pass(
            "kubectl",
            v.lines().next().unwrap_or(v.trim()).trim().to_string(),
        ),
    }
}

fn check_sops() -> CheckResult {
    match run_cmd("sops", &["--version"]) {
        None => {
            if deploy_dir_present() {
                CheckResult::fail(
                    "sops",
                    "not found",
                    "Install sops (https://github.com/getsops/sops) to decrypt deploy/secrets.*.yaml.",
                )
            } else {
                CheckResult::warn(
                    "sops",
                    "not found",
                    "Needed to encrypt/decrypt `erno deploy` secrets.",
                )
            }
        }
        Some(v) => CheckResult::pass(
            "sops",
            v.lines().next().unwrap_or(v.trim()).trim().to_string(),
        ),
    }
}

fn check_age() -> CheckResult {
    match run_cmd("age-keygen", &["--version"]).or_else(|| run_cmd("age", &["--version"])) {
        None => CheckResult::warn(
            "age",
            "not found",
            "Install age (https://age-encryption.org) for SOPS key material.",
        ),
        Some(v) => CheckResult::pass(
            "age",
            v.lines().next().unwrap_or(v.trim()).trim().to_string(),
        ),
    }
}

fn check_deploy_layout() -> Option<CheckResult> {
    let has_new = std::path::Path::new("deploy/config.toml").exists();
    let has_old = std::path::Path::new("chart").is_dir();
    match (has_new, has_old) {
        (false, true) => Some(CheckResult::warn(
            "deploy/",
            "this project still has a Helm chart/",
            "Run: erno deploy migrate",
        )),
        (true, true) => Some(CheckResult::warn(
            "deploy/",
            "chart/ leftover next to deploy/",
            "Remove chart/ after a successful install: git rm -r chart",
        )),
        (true, false) => Some(CheckResult::pass("deploy/", "config.toml found")),
        (false, false) => None,
    }
}

fn deploy_dir_present() -> bool {
    std::path::Path::new("deploy/config.toml").exists()
        || std::path::Path::new("monitoring/deploy/config.toml").exists()
}

fn check_prometheus() -> CheckResult {
    match run_cmd("prometheus", &["--version"]) {
        None => CheckResult::fail(
            "prometheus",
            "not found",
            "Install Prometheus for `erno dev`:\n\
             https://prometheus.io/docs/prometheus/latest/installation/\n\
             Or pass --no-prometheus to `erno dev`.",
        ),
        Some(v) => CheckResult::pass(
            "prometheus",
            v.lines().next().unwrap_or(v.trim()).to_string(),
        ),
    }
}

fn check_tempo() -> CheckResult {
    match run_cmd("tempo", &["-version"]).or_else(|| run_cmd("tempo", &["--version"])) {
        None => CheckResult::fail(
            "tempo",
            "not found",
            "Install Tempo for `erno dev`:\n\
             https://grafana.com/docs/tempo/latest/setup/\n\
             Or pass --no-tempo to `erno dev`.",
        ),
        Some(v) => CheckResult::pass("tempo", v.lines().next().unwrap_or(v.trim()).to_string()),
    }
}

fn check_loki() -> CheckResult {
    match super::dev::loki::probe() {
        super::dev::loki::Binary::Grafana { version } => CheckResult::pass("loki", version),
        super::dev::loki::Binary::Missing => CheckResult::fail(
            "loki",
            "not found",
            "Install Grafana Loki for `erno dev`:\n\
             https://grafana.com/docs/loki/latest/setup/install/\n\
             Or pass --no-loki to `erno dev`.",
        ),
        super::dev::loki::Binary::Other { summary } => CheckResult::fail(
            "loki",
            format!("not Grafana Loki ({summary})"),
            "Debian/Ubuntu's `loki` package is MCMC linkage analysis, not Grafana Loki.\n\
             Install Grafana Loki for `erno dev`:\n\
             https://grafana.com/docs/loki/latest/setup/install/\n\
             Or pass --no-loki to `erno dev`.",
        ),
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
