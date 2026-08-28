//! Registering an application with the monitoring collector.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! One collector watches every Erno application in an organisation, and each
//! application is a *project* on it with its own ingest tokens. Creating that
//! project is a deliberate step rather than something that happens on first
//! report: a typo'd URL would otherwise mint junk projects, and the tokens have
//! to land in this repository's files where a human can see them.
//!
//! What this writes is deliberately narrow. It fills the browser token into
//! `app/src/environments/`, and the server token into `deploy/secrets.example.yaml`
//! — never into `api/config/development.toml`. Pointing a laptop at the shared
//! collector would file local panics against production's issues, because
//! fingerprints ignore `environment`. That is a decision a developer makes by
//! editing one line, not a side effect of registering.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::Value;

use crate::ui;

#[derive(Debug, Subcommand)]
pub enum MonitoringCommands {
    /// Register this application with a collector
    Add(AddArgs),
    /// List the projects a collector knows about
    List(ConnectArgs),
    /// Issue a new ingest token, invalidating the old one
    RotateToken(RotateArgs),
}

/// How to reach a collector, and as whom.
#[derive(Debug, Args, Clone)]
pub struct ConnectArgs {
    /// Collector base URL, e.g. https://monitoring.example.com
    #[arg(long)]
    pub url: String,
    /// Operator username. Falls back to $ERNO_OPERATOR_USER, then a prompt.
    #[arg(long)]
    pub user: Option<String>,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Project slug. Defaults to this application's crate name.
    pub slug: Option<String>,
    #[command(flatten)]
    pub connect: ConnectArgs,
    /// host:port Prometheus should scrape for this application's /metrics
    #[arg(long)]
    pub scrape_target: Option<String>,
    /// http or https (default https)
    #[arg(long)]
    pub scrape_scheme: Option<String>,
    /// Bearer token Prometheus presents when scraping
    #[arg(long)]
    pub metrics_token: Option<String>,
}

#[derive(Debug, Args)]
pub struct RotateArgs {
    /// Project slug. Defaults to this application's crate name.
    pub slug: Option<String>,
    #[command(flatten)]
    pub connect: ConnectArgs,
    /// Rotate the trusted server-to-server token
    #[arg(long)]
    pub server: bool,
    /// Rotate the public browser token
    #[arg(long)]
    pub browser: bool,
}

/// Origins the collector must allow for this application's browser reports.
///
/// Browser ingest is cross-origin and a missing origin fails *silently* — the
/// reports simply stop — so this errs towards including the development ports
/// as well as the deployed hosts.
pub fn default_cors_origins(deploy_config: Option<&str>, has_capacitor: bool) -> Vec<String> {
    let mut origins = Vec::new();
    for host in deploy_config
        .map(hosts_from_deploy_config)
        .unwrap_or_default()
    {
        origins.push(format!("https://{host}"));
    }
    origins.push("http://localhost:4200".to_string());
    origins.push("http://localhost:4300".to_string());
    if has_capacitor {
        // A Capacitor build is not served over http at all; its WebView sends
        // these instead, and without them a device reports nothing.
        origins.push("capacitor://localhost".to_string());
        origins.push("ionic://localhost".to_string());
    }
    origins.dedup();
    origins
}

/// `hosts.app`, `hosts.admin` and `hosts.www` out of a deploy config.
///
/// A hand-rolled read rather than a full parse: this wants three strings out of
/// a file the deploy module already owns the schema for, and only to suggest
/// defaults an operator can edit.
fn hosts_from_deploy_config(config: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    let mut in_hosts = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_hosts = line.ends_with(".hosts]") || line == "[hosts]";
            continue;
        }
        if !in_hosts {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !matches!(key.trim(), "app" | "admin" | "www") {
            continue;
        }
        let host = value.trim().trim_matches('"').trim();
        if !host.is_empty() {
            hosts.push(host.to_string());
        }
    }
    hosts
}

/// Put `errorReporting` on an Angular `environment` object.
///
/// `None` when the file already carries a non-empty `key`: that token is in use
/// by a deployed build, and replacing it would silently stop those reports.
pub fn fill_error_reporting(source: &str, endpoint: &str, key: &str) -> Option<String> {
    if has_live_key(source) {
        return None;
    }
    let block =
        format!("  errorReporting: {{\n    endpoint: '{endpoint}',\n    key: '{key}',\n  }},\n");

    if let Some(existing) = error_reporting_span(source) {
        let mut out = String::with_capacity(source.len() + block.len());
        out.push_str(&source[..existing.0]);
        out.push_str(&block);
        out.push_str(&source[existing.1..]);
        return Some(out);
    }

    // No block yet: add one as the last member of the object literal.
    let close = source.rfind("};")?;
    let head = &source[..close];
    let separator = if head.trim_end().ends_with(',') || head.trim_end().ends_with('{') {
        ""
    } else {
        ",\n"
    };
    Some(format!("{head}{separator}{block}{}", &source[close..]))
}

/// Whether an `errorReporting.key` already holds a token.
///
/// Strips exactly one opening quote and asks whether the closing quote comes
/// straight after. Trimming the quote *character* would eat both of `''` and
/// read an empty key as a live one.
fn has_live_key(source: &str) -> bool {
    let Some((start, end)) = error_reporting_span(source) else {
        return false;
    };
    let Some((_, rest)) = source[start..end].split_once("key:") else {
        return false;
    };
    let mut chars = rest.trim_start().chars();
    let Some(quote) = chars.next().filter(|c| *c == '\'' || *c == '"') else {
        // Not a string literal at all — an expression, or something a person
        // hand-edited. Leave it alone either way.
        return true;
    };
    chars.next() != Some(quote)
}

/// Byte range of the existing `errorReporting: { … },` member, if any.
fn error_reporting_span(source: &str) -> Option<(usize, usize)> {
    let start = source.find("errorReporting:")?;
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let open = source[start..].find('{')? + start;
    let close = source[open..].find('}')? + open + 1;
    // Take a trailing comma and newline with it, so replacing leaves no stray
    // punctuation behind.
    let mut end = close;
    let rest = &source[end..];
    if rest.starts_with(',') {
        end += 1;
    }
    if source[end..].starts_with('\n') {
        end += 1;
    }
    Some((line_start, end))
}

/// Fill an empty `ingest_token: ""` in a secrets file.
///
/// `None` when the key is absent or already set — an in-use token belongs to a
/// running deployment and is never overwritten.
pub fn fill_ingest_token(content: &str, token: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut filled = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if !filled && (trimmed == "ingest_token: \"\"" || trimmed == "ingest_token: ''") {
            let indent = &line[..line.len() - line.trim_start().len()];
            out.push(format!("{indent}ingest_token: \"{token}\""));
            filled = true;
        } else {
            out.push(line.to_string());
        }
    }
    filled.then(|| {
        let mut joined = out.join("\n");
        if content.ends_with('\n') {
            joined.push('\n');
        }
        joined
    })
}

/// The environment files an Angular app keeps its configuration in.
fn environment_files(root: &Path) -> Vec<PathBuf> {
    ["environment.ts", "environment.prod.ts"]
        .iter()
        .map(|name| root.join("app/src/environments").join(name))
        .filter(|p| p.is_file())
        .collect()
}

pub async fn handle(command: MonitoringCommands) -> ui::Cmd {
    match command {
        MonitoringCommands::Add(args) => add(args).await,
        MonitoringCommands::List(args) => list(args).await,
        MonitoringCommands::RotateToken(args) => rotate(args).await,
    }
}

/// Operator credentials, from flags, environment, or a prompt.
fn credentials(connect: &ConnectArgs) -> (String, String) {
    let user = connect
        .user
        .clone()
        .or_else(|| std::env::var("ERNO_OPERATOR_USER").ok())
        .unwrap_or_else(|| ui::prompt("Operator username", "admin"));
    let password = std::env::var("ERNO_OPERATOR_PASSWORD")
        .unwrap_or_else(|_| ui::prompt("Operator password", ""));
    (user, password)
}

fn base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

async fn request(
    method: reqwest::Method,
    url: &str,
    connect: &ConnectArgs,
    body: Option<Value>,
) -> Result<Value, String> {
    let (user, password) = credentials(connect);
    let client = reqwest::Client::new();
    let mut req = client.request(method, url).basic_auth(user, Some(password));
    if let Some(body) = body {
        req = req.json(&body);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("could not reach the collector at {url}: {e}"))?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or(text);
        return Err(match status.as_u16() {
            401 => "the collector rejected those operator credentials".to_string(),
            404 => "no such project on that collector".to_string(),
            409 => format!("{detail} — pick another, or `erno monitoring list --url …`"),
            _ => format!("collector returned {status}: {detail}"),
        });
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn add(args: AddArgs) -> ui::Cmd {
    let root = std::env::current_dir().map_err(|e| format!("cannot read the directory: {e}"))?;
    if !root.join("api/Cargo.toml").is_file() {
        return Err("run this from an application root (the directory with api/)".into());
    }
    let slug = args
        .slug
        .clone()
        .unwrap_or_else(crate::deploy::read_project_name);

    let deploy_config = std::fs::read_to_string(root.join("deploy/config.toml")).ok();
    let has_capacitor = root.join("app/capacitor.config.ts").is_file();
    let origins = default_cors_origins(deploy_config.as_deref(), has_capacitor);

    ui::section(ui::icon::CLOUD, format!("Registering '{slug}'"));

    let body = serde_json::json!({
        "slug": slug,
        "name": slug,
        "cors_origins": origins,
        "scrape_target": args.scrape_target.clone().unwrap_or_default(),
        "scrape_scheme": args.scrape_scheme.clone().unwrap_or_else(|| "https".into()),
        "scrape_metrics_token": args.metrics_token.clone().unwrap_or_default(),
    });
    let base = base_url(&args.connect.url);
    let created = request(
        reqwest::Method::POST,
        &format!("{base}/api/collector/projects"),
        &args.connect,
        Some(body),
    )
    .await?;

    let server_token = created["server_token"].as_str().unwrap_or_default();
    let browser_token = created["browser_token"].as_str().unwrap_or_default();
    ui::ok(format!("project {slug} created"));

    write_tokens(&root, &base, server_token, browser_token);
    report_scrape(args.scrape_target.as_deref());
    print_add_next_steps(&origins, server_token, has_capacitor);
    Ok(())
}

/// Put the two tokens where the application reads them.
fn write_tokens(root: &Path, base: &str, server_token: &str, browser_token: &str) {
    let endpoint = format!("{base}/api/errors");
    for path in environment_files(root) {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let shown = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        match fill_error_reporting(&source, &endpoint, browser_token) {
            Some(updated) => {
                if std::fs::write(&path, updated).is_ok() {
                    ui::ok(shown);
                }
            }
            None => {
                ui::info(format!(
                    "{shown} already has an errorReporting key — left as is"
                ));
            }
        }
    }

    let secrets = root.join("deploy/secrets.example.yaml");
    let Ok(content) = std::fs::read_to_string(&secrets) else {
        return;
    };
    match fill_ingest_token(&content, server_token) {
        Some(updated) => {
            if std::fs::write(&secrets, updated).is_ok() {
                ui::ok("deploy/secrets.example.yaml");
            }
        }
        None => ui::info("deploy/secrets.example.yaml already has an ingest_token — left as is"),
    }
}

fn report_scrape(target: Option<&str>) {
    if target.is_some_and(|t| !t.trim().is_empty()) {
        return;
    }
    ui::warn("no scrape target — Prometheus will not scrape this application");
    ui::detail(
        "Pass --scrape-target host:port, or set it on the project in the console.\n\
         Errors, uptime and alerts work without it; metrics do not.",
    );
}

fn print_add_next_steps(origins: &[String], server_token: &str, has_capacitor: bool) {
    ui::blank();
    ui::info("Allowed browser origins:");
    for origin in origins {
        ui::detail(origin);
    }
    if !has_capacitor {
        ui::detail("No capacitor.config.ts found, so no device origins were added.");
    }
    ui::blank();
    ui::info("For the release webhook in CI:");
    ui::detail(format!("ERNO_INGEST_TOKEN={server_token}"));
    ui::blank();
    ui::info("Local development still reports nowhere, on purpose.");
    ui::detail(
        "Fingerprints ignore the environment, so a laptop pointed at this\n\
         collector files its panics against production's issues. Set\n\
         [error_reporting] collector_url in api/config/development.toml only if\n\
         that is what you want.",
    );
    ui::blank();
    ui::info("The admin console is not wired up automatically. To report from it,");
    ui::detail("give its provideErno the same endpoint and key, with X-Erno-Source: admin.");
}

async fn list(args: ConnectArgs) -> ui::Cmd {
    let base = base_url(&args.url);
    let body = request(
        reqwest::Method::GET,
        &format!("{base}/api/collector/projects"),
        &args,
        None,
    )
    .await?;

    let projects = body["projects"].as_array().cloned().unwrap_or_default();
    if projects.is_empty() {
        ui::info("no projects on that collector");
        return Ok(());
    }
    ui::section(ui::icon::CLOUD, format!("{} project(s)", projects.len()));
    for project in projects {
        let slug = project["slug"].as_str().unwrap_or("?");
        let target = project["scrape_target"].as_str().unwrap_or_default();
        ui::info(slug);
        ui::detail(if target.is_empty() {
            "no scrape target".to_string()
        } else {
            format!("scrape {target}")
        });
    }
    Ok(())
}

async fn rotate(args: RotateArgs) -> ui::Cmd {
    let which = match (args.server, args.browser) {
        (true, false) => "server",
        (false, true) => "browser",
        _ => return Err("pass exactly one of --server or --browser".into()),
    };
    let slug = args
        .slug
        .clone()
        .unwrap_or_else(crate::deploy::read_project_name);
    let base = base_url(&args.connect.url);
    let body = request(
        reqwest::Method::POST,
        &format!("{base}/api/collector/projects/{slug}/tokens/{which}"),
        &args.connect,
        None,
    )
    .await?;

    let token = body["token"].as_str().unwrap_or_default();
    ui::ok(format!("{which} token rotated for {slug}"));
    ui::blank();
    ui::detail(token);
    ui::blank();
    ui::warn("the previous token stops working within the collector's cache TTL");
    ui::detail(match which {
        "server" => "Update deploy/secrets.<env>.yaml api.ingest_token and redeploy.",
        _ => "Update app/src/environments/ and ship a new browser build.",
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployed_hosts_and_dev_ports_are_both_allowed() {
        let config = r#"
github_repo = "acme/acme"

[production]
kubernetes_context = "prod"

[production.hosts]
api = "api.example.com"
app = "app.example.com"
admin = "admin.example.com"
www = "example.com"
"#;
        let origins = default_cors_origins(Some(config), false);
        // The API is not a browser origin; the three that serve pages are.
        assert!(origins.contains(&"https://app.example.com".to_string()));
        assert!(origins.contains(&"https://admin.example.com".to_string()));
        assert!(origins.contains(&"https://example.com".to_string()));
        assert!(!origins.iter().any(|o| o.contains("api.example.com")));
        assert!(origins.contains(&"http://localhost:4200".to_string()));
        assert!(!origins.iter().any(|o| o.starts_with("capacitor")));
    }

    #[test]
    fn a_capacitor_app_gets_its_device_origins() {
        let origins = default_cors_origins(None, true);
        assert!(origins.contains(&"capacitor://localhost".to_string()));
        assert!(origins.contains(&"ionic://localhost".to_string()));
    }

    #[test]
    fn an_environment_without_a_block_gains_one() {
        let source = "export const environment = {\n  production: false,\n  apiUrl: 'http://localhost:3000',\n};\n";
        let out = fill_error_reporting(source, "https://m.test/api/errors", "ernb_x")
            .expect("should fill");
        assert!(out.contains("errorReporting: {"));
        assert!(out.contains("endpoint: 'https://m.test/api/errors'"));
        assert!(out.contains("key: 'ernb_x'"));
        // The object must still parse: the previous member keeps its comma.
        assert!(out.contains("apiUrl: 'http://localhost:3000',"));
        assert!(out.trim_end().ends_with("};"));
    }

    #[test]
    fn an_empty_block_is_filled_in_place() {
        let source = "export const environment = {\n  production: false,\n  errorReporting: {\n    endpoint: '',\n    key: '',\n  },\n};\n";
        let out = fill_error_reporting(source, "https://m.test/api/errors", "ernb_x")
            .expect("should fill");
        assert!(out.contains("key: 'ernb_x'"));
        assert_eq!(
            out.matches("errorReporting").count(),
            1,
            "no duplicate block"
        );
    }

    /// That token is in a shipped browser build. Replacing it would stop those
    /// reports arriving, and nothing would say so.
    #[test]
    fn a_key_already_in_use_is_never_overwritten() {
        let source = "export const environment = {\n  errorReporting: {\n    endpoint: 'https://old/api/errors',\n    key: 'ernb_live',\n  },\n};\n";
        assert!(fill_error_reporting(source, "https://m.test/api/errors", "ernb_new").is_none());
    }

    #[test]
    fn an_empty_ingest_token_is_filled_and_a_live_one_is_left_alone() {
        let empty = "api:\n  ingest_token: \"\"\n";
        let filled = fill_ingest_token(empty, "erns_x").expect("should fill");
        assert!(filled.contains("ingest_token: \"erns_x\""));
        // Indentation survives, or the YAML stops being valid.
        assert!(filled.contains("  ingest_token:"));

        assert!(fill_ingest_token("api:\n  ingest_token: \"live\"\n", "erns_x").is_none());
        assert!(fill_ingest_token("api: {}\n", "erns_x").is_none());
    }
}
