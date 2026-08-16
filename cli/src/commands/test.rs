use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use clap::Args;
use serde::Deserialize;

use crate::commands::dev::resolve_project_root;
use crate::global_config::GlobalConfig;

#[derive(Args, Debug, Default)]
pub struct TestArgs {
    /// Run only the API `cargo test` suite
    #[arg(long)]
    pub api: bool,
    /// Run only the app Karma suite (unit + feature)
    #[arg(long)]
    pub app: bool,
    /// Run only Playwright end-to-end tests
    #[arg(long)]
    pub e2e: bool,
    /// Skip Playwright even when `e2e/` exists
    #[arg(long)]
    pub no_e2e: bool,
    /// Named extra suite from `.erno/test.toml` (repeatable)
    #[arg(long)]
    pub suite: Vec<String>,
    /// Stop after the first failing suite
    #[arg(long)]
    pub fail_fast: bool,
    /// Arguments forwarded to the single selected suite's runner
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suite {
    pub name: String,
    pub kind: SuiteKind,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuiteKind {
    Api,
    App,
    E2e,
    Extra {
        dir: PathBuf,
        command: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Deserialize, Default)]
struct ExtraFile {
    #[serde(default)]
    suite: Vec<ExtraSuite>,
}

#[derive(Debug, Deserialize)]
struct ExtraSuite {
    name: String,
    dir: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_true")]
    default: bool,
}

fn default_true() -> bool {
    true
}

pub fn discover_suites(root: &Path) -> Result<Vec<Suite>, String> {
    let mut suites = Vec::new();
    if root.join("api").join("Cargo.toml").is_file() {
        suites.push(Suite {
            name: "api".into(),
            kind: SuiteKind::Api,
            default: true,
        });
    }
    if root.join("app").join("package.json").is_file() {
        suites.push(Suite {
            name: "app".into(),
            kind: SuiteKind::App,
            default: true,
        });
    }
    suites.extend(load_extras(root)?);
    if playwright_config(root).is_some() {
        suites.push(Suite {
            name: "e2e".into(),
            kind: SuiteKind::E2e,
            default: true,
        });
    }
    Ok(suites)
}

fn playwright_config(root: &Path) -> Option<PathBuf> {
    for rel in ["e2e/playwright.config.ts", "playwright.config.ts"] {
        let p = root.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn load_extras(root: &Path) -> Result<Vec<Suite>, String> {
    let path = root.join(".erno").join("test.toml");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let parsed: ExtraFile =
        toml_from_str(&raw).map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    Ok(parsed
        .suite
        .into_iter()
        .map(|s| Suite {
            name: s.name,
            kind: SuiteKind::Extra {
                dir: root.join(s.dir),
                command: s.command,
                args: s.args,
            },
            default: s.default,
        })
        .collect())
}

/// Minimal TOML table parser for `[[suite]]` files — avoids a CLI toml crate.
fn toml_from_str(raw: &str) -> Result<ExtraFile, String> {
    // Use config-rs: write to a temp-less in-memory approach via File source
    // is path-based. Parse by hand for the small schema.
    let mut suites = Vec::new();
    let mut current: Option<ExtraSuite> = None;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[suite]]" {
            if let Some(s) = current.take() {
                suites.push(s);
            }
            current = Some(ExtraSuite {
                name: String::new(),
                dir: String::new(),
                command: String::new(),
                args: Vec::new(),
                default: true,
            });
            continue;
        }
        let Some(entry) = current.as_mut() else {
            return Err(format!("unexpected line outside [[suite]]: {line}"));
        };
        let Some((k, v)) = line.split_once('=') else {
            return Err(format!("expected key = value, got {line}"));
        };
        let k = k.trim();
        let v = v.trim();
        match k {
            "name" => entry.name = unquote(v)?,
            "dir" => entry.dir = unquote(v)?,
            "command" => entry.command = unquote(v)?,
            "default" => entry.default = v.parse::<bool>().map_err(|e| e.to_string())?,
            "args" => entry.args = parse_string_array(v)?,
            _ => return Err(format!("unknown suite key {k}")),
        }
    }
    if let Some(s) = current {
        suites.push(s);
    }
    for s in &suites {
        if s.name.is_empty() || s.dir.is_empty() || s.command.is_empty() {
            return Err("each [[suite]] needs name, dir, and command".into());
        }
    }
    Ok(ExtraFile { suite: suites })
}

fn unquote(v: &str) -> Result<String, String> {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Ok(inner.to_string())
    } else {
        Err(format!("expected quoted string, got {v}"))
    }
}

fn parse_string_array(v: &str) -> Result<Vec<String>, String> {
    let v = v.trim();
    let inner = v
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| format!("expected array, got {v}"))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    inner.split(',').map(|part| unquote(part.trim())).collect()
}

pub fn select_suites<'a>(all: &'a [Suite], args: &TestArgs) -> Result<Vec<&'a Suite>, String> {
    let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
    for asked in &args.suite {
        if !all.iter().any(|s| s.name == *asked) {
            return Err(format!(
                "unknown suite '{asked}'. Known: {}",
                names.join(", ")
            ));
        }
    }
    let filtered: Vec<&Suite> = if args.api || args.app || args.e2e || !args.suite.is_empty() {
        all.iter()
            .filter(|s| match s.kind {
                SuiteKind::Api => args.api || args.suite.iter().any(|n| n == "api"),
                SuiteKind::App => args.app || args.suite.iter().any(|n| n == "app"),
                SuiteKind::E2e => {
                    (args.e2e || args.suite.iter().any(|n| n == "e2e")) && !args.no_e2e
                }
                SuiteKind::Extra { .. } => args.suite.iter().any(|n| n == &s.name),
            })
            .collect()
    } else {
        all.iter()
            .filter(|s| s.default && !(args.no_e2e && matches!(s.kind, SuiteKind::E2e)))
            .collect()
    };
    if filtered.is_empty() {
        return Err("no suites selected".into());
    }
    if !args.rest.is_empty() && filtered.len() != 1 {
        return Err(
            "pass-through arguments require exactly one suite (use --api, --app, --e2e, or --suite)"
                .into(),
        );
    }
    Ok(filtered)
}

pub async fn handle_test(args: TestArgs) {
    let root = resolve_project_root(None);
    let all = match discover_suites(&root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌  {e}");
            std::process::exit(1);
        }
    };
    let selected = match select_suites(&all, &args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌  {e}");
            std::process::exit(1);
        }
    };

    let needs_db = selected
        .iter()
        .any(|s| matches!(s.kind, SuiteKind::Api | SuiteKind::E2e));
    if needs_db {
        if let Err(e) = ensure_test_database(&root).await {
            eprintln!("❌  {e}");
            std::process::exit(1);
        }
    }

    let mut results: Vec<(String, bool)> = Vec::new();
    for suite in &selected {
        println!("\n── {} ──", suite.name);
        let ok = run_suite(&root, suite, &args.rest);
        results.push((suite.name.clone(), ok));
        if !ok && args.fail_fast {
            break;
        }
    }

    println!();
    let mut failed = false;
    for (name, ok) in &results {
        if *ok {
            println!("  {name:<12} ok");
        } else {
            println!("  {name:<12} fail");
            failed = true;
        }
    }
    if failed {
        std::process::exit(1);
    }
}

fn run_suite(root: &Path, suite: &Suite, rest: &[String]) -> bool {
    match &suite.kind {
        SuiteKind::Api => {
            let mut cmd = Command::new("cargo");
            cmd.arg("test").current_dir(root.join("api"));
            cmd.args(rest);
            run_prefixed(&mut cmd, "api")
        }
        SuiteKind::App => {
            let app = root.join("app");
            if !ensure_npm_modules(&app, "app") {
                return false;
            }
            let pkg = std::fs::read_to_string(app.join("package.json")).unwrap_or_default();
            let mut cmd = Command::new("npm");
            if pkg.contains("\"test:ci\"") {
                cmd.args(["run", "test:ci"]);
            } else {
                cmd.args(["test", "--", "--watch=false", "--browsers=ChromeHeadless"]);
            }
            cmd.current_dir(&app);
            if !rest.is_empty() {
                cmd.arg("--").args(rest);
            }
            run_prefixed(&mut cmd, "app")
        }
        SuiteKind::E2e => run_e2e(root, rest),
        SuiteKind::Extra { dir, command, args } => {
            let mut cmd = Command::new(command);
            cmd.args(args).args(rest).current_dir(dir);
            run_prefixed(&mut cmd, &suite.name)
        }
    }
}

fn ensure_npm_modules(dir: &Path, label: &str) -> bool {
    if dir.join("node_modules").is_dir() {
        return true;
    }
    if !dir.join("package.json").is_file() {
        eprintln!("[{label}] no package.json in {}", dir.display());
        return false;
    }
    eprintln!("[{label}] npm install in {}", dir.display());
    let mut cmd = Command::new("npm");
    cmd.arg("install").current_dir(dir);
    let ok = run_prefixed(&mut cmd, label);
    if !ok {
        eprintln!("[{label}] npm install failed in {}", dir.display());
    }
    ok
}

fn run_prefixed(cmd: &mut Command, label: &str) -> bool {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[{label}] failed to start: {e}");
            return false;
        }
    };
    if let Some(out) = child.stdout.take() {
        prefix_pipe(out, label);
    }
    if let Some(err) = child.stderr.take() {
        prefix_pipe(err, label);
    }
    match child.wait() {
        Ok(status) => status.success(),
        Err(e) => {
            eprintln!("[{label}] wait failed: {e}");
            false
        }
    }
}

fn prefix_pipe<R: std::io::Read + Send + 'static>(pipe: R, label: &str) {
    use std::io::BufRead;
    let label = label.to_string();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(pipe);
        for line in reader.lines().map_while(Result::ok) {
            println!("[{label}] {line}");
        }
    });
}

fn run_e2e(root: &Path, rest: &[String]) -> bool {
    let api_port = match free_port() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[e2e] {e}");
            return false;
        }
    };
    let app_port = match free_port() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[e2e] {e}");
            return false;
        }
    };
    let api_url = format!("http://127.0.0.1:{api_port}");
    let app_url = format!("http://127.0.0.1:{app_port}");
    let cors = format!("http://127.0.0.1:{app_port},http://localhost:{app_port}");

    let app_dir = root.join("app");
    if app_dir.join("package.json").is_file() && !ensure_npm_modules(&app_dir, "app") {
        return false;
    }

    let api_dir = root.join("api");
    let mut api = match Command::new("cargo")
        .arg("run")
        .current_dir(&api_dir)
        .env("APP_ENVIRONMENT", "test")
        .env("APP__SERVER__PORT", api_port.to_string())
        .env("APP__API_URL", &api_url)
        .env("APP__APP_URL", &app_url)
        .env("APP__DATABASE__POOL_SIZE", "10")
        .env("ERNO_DEV_CORS_ORIGINS", &cors)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[e2e] failed to start API: {e}");
            return false;
        }
    };
    if let Some(out) = api.stdout.take() {
        prefix_pipe(out, "api");
    }
    if let Some(err) = api.stderr.take() {
        prefix_pipe(err, "api");
    }

    let liveness = format!("{api_url}/liveness");
    eprintln!("[e2e] API {api_url}");
    eprintln!("[e2e] app {app_url}");
    eprintln!("[e2e] waiting for {liveness}");
    let ready = wait_for_http_while(&liveness, 90, || child_running(&mut api));
    if !ready {
        if !child_running(&mut api) {
            eprintln!("[e2e] API process exited before /liveness answered — not using a leftover listener");
        } else {
            eprintln!("[e2e] API did not become ready on {api_url}");
        }
        let _ = api.kill();
        let _ = api.wait();
        return false;
    }
    if !child_running(&mut api) {
        eprintln!("[e2e] API process exited; /liveness was another process. Refusing to continue.");
        return false;
    }

    let e2e = if root.join("e2e").is_dir() {
        root.join("e2e")
    } else {
        root.to_path_buf()
    };
    if e2e.join("package.json").is_file() && !ensure_npm_modules(&e2e, "e2e") {
        let _ = api.kill();
        let _ = api.wait();
        return false;
    }
    let pw_bin = e2e.join("node_modules").join(".bin").join("playwright");
    if !pw_bin.is_file() {
        eprintln!(
            "[e2e] Playwright CLI is missing at {}.\n      Add @playwright/test to e2e/package.json and run: cd e2e && npm install && npx playwright install chromium",
            pw_bin.display()
        );
        let _ = api.kill();
        let _ = api.wait();
        return false;
    }
    // Browsers are a one-time download; skip if already present.
    let _ = Command::new(&pw_bin)
        .args(["install", "chromium"])
        .current_dir(&e2e)
        .status();
    let mut pw = Command::new(&pw_bin);
    pw.arg("test")
        .current_dir(&e2e)
        .env("API_URL", &api_url)
        .env("APP_URL", &app_url)
        .env("ERNO_E2E", "1")
        .args(rest);
    let ok = run_prefixed(&mut pw, "e2e");
    let _ = api.kill();
    let _ = api.wait();
    ok
}

/// Bind `127.0.0.1:0` and return the kernel-assigned port. The listener is
/// dropped immediately; the caller must bind again (small race, unlike a
/// dice-roll in a fixed range which collides even when the port is taken).
fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("could not allocate a free port: {e}"))?;
    listener.set_nonblocking(true).ok();
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|e| format!("could not read bound port: {e}"))
}

fn child_running(child: &mut Child) -> bool {
    match child.try_wait() {
        Ok(None) => true,
        Ok(Some(_)) | Err(_) => false,
    }
}

fn wait_for_http_while(url: &str, seconds: u64, mut still_running: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    while std::time::Instant::now() < deadline {
        if !still_running() {
            return false;
        }
        if let Ok(status) = Command::new("curl")
            .args(["-sf", "-o", "/dev/null", "-w", "%{http_code}", url])
            .output()
        {
            if status.status.success() {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    false
}

fn test_database_url(root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(root.join("api/config/test.toml")).ok()?;
    let mut in_database = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_database = line == "[database]";
            continue;
        }
        if in_database {
            if let Some(rest) = line.strip_prefix("url") {
                let rest = rest.trim().trim_start_matches('=').trim();
                return unquote(rest).ok();
            }
        }
    }
    None
}

fn database_name(url: &str) -> Option<String> {
    let after = url.rsplit('/').next()?;
    let name = after.split('?').next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn database_user(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let creds = rest.split('@').next()?;
    let user = creds.split(':').next()?.trim();
    if user.is_empty() {
        None
    } else {
        Some(user.to_string())
    }
}

async fn ensure_test_database(root: &Path) -> Result<(), String> {
    let ready = Command::new("pg_isready").status();
    match ready {
        Ok(s) if s.success() => {}
        _ => return Err("PostgreSQL is not running (`pg_isready` failed)".into()),
    }
    let url = test_database_url(root)
        .ok_or_else(|| "could not read [database].url from api/config/test.toml".to_string())?;
    let db =
        database_name(&url).ok_or_else(|| format!("could not parse database name from {url}"))?;
    let owner = database_user(&url);

    let config = match GlobalConfig::load() {
        Ok(c) => Some(c),
        Err(_) => None,
    };

    let exists = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .is_ok();
    if !exists {
        let admin_url = config.as_ref().map(|c| c.postgres.admin_url.as_str()).ok_or_else(|| {
            format!(
                "test database `{db}` is missing and ~/.erno/config.toml was not found.\n      Run `erno setup`, then: createdb {db}"
            )
        })?;
        let (client, connection) = tokio_postgres::connect(admin_url, tokio_postgres::NoTls)
            .await
            .map_err(|e| format!("could not connect as admin to create `{db}`: {e}"))?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let create_sql = match &owner {
            Some(user) => format!("CREATE DATABASE {db} OWNER {user}"),
            None => format!("CREATE DATABASE {db}"),
        };
        match client.execute(&create_sql, &[]).await {
            Ok(_) => println!("  created database {db}"),
            Err(e) => {
                let msg = e
                    .as_db_error()
                    .map(|d| d.message().to_string())
                    .unwrap_or_else(|| e.to_string());
                if msg.contains("already exists") {
                    // continue to grants
                } else if owner.is_some() {
                    // Admin often cannot SET ROLE to the app user. Create unowned, then GRANT.
                    client
                        .execute(&format!("CREATE DATABASE {db}"), &[])
                        .await
                        .map_err(|e2| format!("could not create `{db}`: {e2}"))?;
                    println!("  created database {db} (admin-owned; granting to app role)");
                } else {
                    return Err(format!("could not create `{db}`: {msg}"));
                }
            }
        }
        if let Some(user) = &owner {
            let _ = client
                .execute(
                    &format!("GRANT ALL PRIVILEGES ON DATABASE {db} TO {user}"),
                    &[],
                )
                .await;
        }
    }

    if let (Some(config), Some(user)) = (config.as_ref(), owner.as_ref()) {
        grant_public_schema(&config.postgres.admin_url, &db, user).await;
    }

    if tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .is_ok()
    {
        return Ok(());
    }
    Err(format!(
        "test database `{db}` exists but the app role cannot connect. Grant access or: createdb -O {db} {db}"
    ))
}

fn with_db(admin_url: &str, db: &str) -> String {
    match admin_url.rfind('/') {
        Some(pos) => format!("{}/{}", &admin_url[..pos], db),
        None => format!("{admin_url}/{db}"),
    }
}

async fn grant_public_schema(admin_url: &str, db: &str, user: &str) {
    let Ok((client, connection)) =
        tokio_postgres::connect(&with_db(admin_url, db), tokio_postgres::NoTls).await
    else {
        return;
    };
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .execute(&format!("GRANT ALL ON SCHEMA public TO {user}"), &[])
        .await;
    let _ = client
        .execute(&format!("ALTER SCHEMA public OWNER TO {user}"), &[])
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "erno-test-{}-{}-{suffix}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn discovers_api_and_app() {
        let root = temp("disc");
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join("api/Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app/package.json"), "{}\n").unwrap();
        let suites = discover_suites(&root).unwrap();
        assert_eq!(
            suites.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["api", "app"]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn extras_run_before_e2e() {
        let root = temp("order");
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join("api/Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app/package.json"), "{}\n").unwrap();
        fs::create_dir_all(root.join("e2e")).unwrap();
        fs::write(root.join("e2e/playwright.config.ts"), "export default {}\n").unwrap();
        fs::create_dir_all(root.join(".erno")).unwrap();
        fs::write(
            root.join(".erno/test.toml"),
            r#"
[[suite]]
name = "puzzles"
dir = "puzzles"
command = "cargo"
args = ["test"]
default = true
"#,
        )
        .unwrap();
        let names: Vec<_> = discover_suites(&root)
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["api", "app", "puzzles", "e2e"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn free_port_is_nonzero_and_two_calls_differ() {
        let a = free_port().expect("first free port");
        let b = free_port().expect("second free port");
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn extras_default_and_opt_in() {
        let raw = r#"
[[suite]]
name = "puzzles"
dir = "puzzles"
command = "cargo"
args = ["test"]
default = true

[[suite]]
name = "vision"
dir = "vision"
command = "cargo"
args = ["test"]
default = false
"#;
        let parsed = toml_from_str(raw).unwrap();
        assert_eq!(parsed.suite.len(), 2);
        assert!(parsed.suite[0].default);
        assert!(!parsed.suite[1].default);
        assert_eq!(parsed.suite[0].args, ["test"]);
    }

    #[test]
    fn select_default_skips_opt_in_and_no_e2e() {
        let all = vec![
            Suite {
                name: "api".into(),
                kind: SuiteKind::Api,
                default: true,
            },
            Suite {
                name: "e2e".into(),
                kind: SuiteKind::E2e,
                default: true,
            },
            Suite {
                name: "vision".into(),
                kind: SuiteKind::Extra {
                    dir: PathBuf::from("vision"),
                    command: "cargo".into(),
                    args: vec!["test".into()],
                },
                default: false,
            },
        ];
        let selected = select_suites(
            &all,
            &TestArgs {
                no_e2e: true,
                ..TestArgs::default()
            },
        )
        .unwrap();
        assert_eq!(
            selected.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["api"]
        );
    }

    #[test]
    fn pass_through_requires_one_suite() {
        let all = vec![
            Suite {
                name: "api".into(),
                kind: SuiteKind::Api,
                default: true,
            },
            Suite {
                name: "app".into(),
                kind: SuiteKind::App,
                default: true,
            },
        ];
        let err = select_suites(
            &all,
            &TestArgs {
                rest: vec!["health".into()],
                ..TestArgs::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("pass-through"));
        let ok = select_suites(
            &all,
            &TestArgs {
                api: true,
                rest: vec!["health".into()],
                ..TestArgs::default()
            },
        )
        .unwrap();
        assert_eq!(ok.len(), 1);
    }

    #[test]
    fn parses_role_from_database_url() {
        assert_eq!(
            database_user("postgres://cubeast_erno:secret@localhost/cubeast_erno_test").as_deref(),
            Some("cubeast_erno")
        );
        assert_eq!(
            database_name("postgres://cubeast_erno:secret@localhost/cubeast_erno_test").as_deref(),
            Some("cubeast_erno_test")
        );
    }

    #[test]
    fn unknown_suite_lists_known() {
        let all = vec![Suite {
            name: "api".into(),
            kind: SuiteKind::Api,
            default: true,
        }];
        let err = select_suites(
            &all,
            &TestArgs {
                suite: vec!["nope".into()],
                ..TestArgs::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("nope"));
        assert!(err.contains("api"));
    }
}
