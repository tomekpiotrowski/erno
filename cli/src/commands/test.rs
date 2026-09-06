use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::commands::dev::resolve_project_root;
use crate::commands::packages::{
    ensure_bun_modules, load_packages, prefix_pipe, run_phase, run_prefixed, select, Phase,
    SelectionArgs,
};
use crate::global_config::GlobalConfig;
use crate::ui;

fn unquote(v: &str) -> Result<String, String> {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Ok(inner.to_string())
    } else {
        Err(format!("expected quoted string, got {v}"))
    }
}

pub async fn handle_test(args: SelectionArgs) -> ui::Cmd {
    let root = resolve_project_root(None)?;
    let all = load_packages(&root)?;
    let selected = select(&all, &args)?;

    // Each package that declares `database` gets its own test database, so a
    // single hardcoded api/config/test.toml is not enough. Duplicates are
    // skipped, since e2e and any package without its own config fall back to
    // the api's.
    let mut ensured: Vec<String> = Vec::new();
    for package in selected.iter().filter(|p| p.database || p.is_e2e()) {
        let config = test_config_path(&root, &package.dir);
        let key = config.display().to_string();
        if ensured.contains(&key) {
            continue;
        }
        ensured.push(key);
        ensure_test_database(&config).await?;
    }

    // The e2e package is not a plain command: it allocates ports, boots the API,
    // and tears it down again. Everything else runs its declared steps.
    let ok = run_phase(
        &root,
        &selected,
        Phase::Test,
        false,
        &args,
        &mut |package| {
            if !package.is_e2e() {
                return None;
            }
            ui::section(ui::icon::TEST, &package.name);
            Some(run_e2e(&root, &args.rest))
        },
    );
    if ok {
        Ok(())
    } else {
        // `run_phase` already printed the per-package summary.
        Err(ui::Failure::Silent)
    }
}

fn run_e2e(root: &Path, rest: &[String]) -> bool {
    let api_port = match free_port() {
        Ok(p) => p,
        Err(e) => {
            ui::prefixed(ui::Stream::Err, "e2e", &e);
            return false;
        }
    };
    let app_port = match free_port() {
        Ok(p) => p,
        Err(e) => {
            ui::prefixed(ui::Stream::Err, "e2e", &e);
            return false;
        }
    };
    let api_url = format!("http://127.0.0.1:{api_port}");
    let app_url = format!("http://127.0.0.1:{app_port}");
    let cors = format!("http://127.0.0.1:{app_port},http://localhost:{app_port}");

    let app_dir = root.join("app");
    if app_dir.join("package.json").is_file() && !ensure_bun_modules(&app_dir, "app") {
        return false;
    }

    let api_dir = root.join("api");
    // Compile first so the 90s /liveness wait is boot, not a cold cargo run.
    let mut build = e2e_api_build_cmd(&api_dir);
    if !run_prefixed(&mut build, "e2e") {
        return false;
    }

    let mut api = match e2e_api_run_cmd(&api_dir, api_port, &api_url, &app_url, &cors).spawn() {
        Ok(c) => c,
        Err(e) => {
            ui::prefixed(
                ui::Stream::Err,
                "e2e",
                &format!("failed to start the API: {e}"),
            );
            return false;
        }
    };
    if let Some(out) = api.stdout.take() {
        prefix_pipe(out, "api", ui::Stream::Out);
    }
    if let Some(err) = api.stderr.take() {
        prefix_pipe(err, "api", ui::Stream::Err);
    }

    let liveness = format!("{api_url}/liveness");
    ui::prefixed(ui::Stream::Err, "e2e", &format!("API {api_url}"));
    ui::prefixed(ui::Stream::Err, "e2e", &format!("app {app_url}"));
    ui::prefixed(ui::Stream::Err, "e2e", &format!("waiting for {liveness}"));
    let ready = wait_for_http_while(&liveness, 90, || child_running(&mut api));
    if !ready {
        if !child_running(&mut api) {
            ui::prefixed(
                ui::Stream::Err,
                "e2e",
                "API process exited before /liveness answered — not using a leftover listener",
            );
        } else {
            ui::prefixed(
                ui::Stream::Err,
                "e2e",
                &format!("API did not become ready on {api_url}"),
            );
        }
        let _ = api.kill();
        let _ = api.wait();
        return false;
    }
    if !child_running(&mut api) {
        ui::prefixed(
            ui::Stream::Err,
            "e2e",
            "API process exited; /liveness was another process — refusing to continue",
        );
        return false;
    }

    let e2e = if root.join("e2e").is_dir() {
        root.join("e2e")
    } else {
        root.to_path_buf()
    };
    if e2e.join("package.json").is_file() && !ensure_bun_modules(&e2e, "e2e") {
        let _ = api.kill();
        let _ = api.wait();
        return false;
    }
    let pw_bin = e2e.join("node_modules").join(".bin").join("playwright");
    if !pw_bin.is_file() {
        ui::prefixed(
            ui::Stream::Err,
            "e2e",
            &format!("Playwright CLI is missing at {}", pw_bin.display()),
        );
        ui::detail(
            "Add @playwright/test to e2e/package.json, then run:\n\
             cd e2e && bun install && bun x playwright install chromium",
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

/// `cargo build` for the API crate, before e2e waits on `/liveness`.
///
/// A cold `cargo run` can spend the whole 90s window compiling Erno from git.
/// Building first keeps that wait for boot.
fn e2e_api_build_cmd(api_dir: &Path) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .current_dir(api_dir)
        .env("APP_ENVIRONMENT", "test");
    cmd
}

/// Incremental `cargo run` after `e2e_api_build_cmd`.
fn e2e_api_run_cmd(
    api_dir: &Path,
    api_port: u16,
    api_url: &str,
    app_url: &str,
    cors: &str,
) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .current_dir(api_dir)
        .env("APP_ENVIRONMENT", "test")
        .env("APP__SERVER__PORT", api_port.to_string())
        .env("APP__API_URL", api_url)
        .env("APP__APP_URL", app_url)
        .env("APP__DATABASE__POOL_SIZE", "10")
        .env("ERNO_DEV_CORS_ORIGINS", cors)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
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

/// Which `config/test.toml` describes a package's test database.
///
/// A package with its own config owns its own database; anything else — `e2e`
/// most of all — runs against the api's.
fn test_config_path(root: &Path, dir: &str) -> std::path::PathBuf {
    let own = root.join(dir).join("config").join("test.toml");
    if own.is_file() {
        own
    } else {
        root.join("api").join("config").join("test.toml")
    }
}

fn test_database_url(config: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(config).ok()?;
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

async fn ensure_test_database(config: &Path) -> Result<(), String> {
    let ready = crate::postgres::pg_isready().status();
    match ready {
        Ok(s) if s.success() => {}
        _ => {
            return Err(format!(
                "PostgreSQL is not running (`pg_isready` failed)\n{}",
                crate::postgres::start_hint()
            ))
        }
    }
    let url = test_database_url(config)
        .ok_or_else(|| format!("could not read [database].url from {}", config.display()))?;
    let db =
        database_name(&url).ok_or_else(|| format!("could not parse database name from {url}"))?;
    let owner = database_user(&url);

    let config = GlobalConfig::load().ok();

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
            Ok(_) => ui::ok(format!("created database {db}")),
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
                    ui::ok(format!(
                        "created database {db} (admin-owned; granting to app role)"
                    ));
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

    #[test]
    fn e2e_builds_the_api_before_running_it() {
        let api = Path::new("/tmp/acme/api");
        let build = e2e_api_build_cmd(api);
        let run = e2e_api_run_cmd(
            api,
            3001,
            "http://127.0.0.1:3001",
            "http://127.0.0.1:4200",
            "",
        );
        let build_dbg = format!("{build:?}");
        let run_dbg = format!("{run:?}");
        assert!(
            build_dbg.contains("\"build\""),
            "e2e must cargo-build first: {build_dbg}"
        );
        assert!(
            !build_dbg.contains("\"run\""),
            "the build step must not be cargo run: {build_dbg}"
        );
        assert!(
            run_dbg.contains("\"run\""),
            "e2e must cargo-run after the build: {run_dbg}"
        );
        assert!(
            run_dbg.contains("APP_ENVIRONMENT") && run_dbg.contains("test"),
            "the run step boots the test environment: {run_dbg}"
        );
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
    fn reads_the_database_url_out_of_test_config() {
        let root = std::env::temp_dir().join(format!(
            "erno-test-cfg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("api/config")).unwrap();
        std::fs::write(
            root.join("api/config/test.toml"),
            "[server]\nport = 3000\n\n[database]\nurl = \"postgres://u:p@localhost/x_test\"\n",
        )
        .unwrap();
        assert_eq!(
            test_database_url(&root.join("api/config/test.toml")).as_deref(),
            Some("postgres://u:p@localhost/x_test")
        );

        // A package with its own config owns its own database; e2e does not,
        // and falls back to the api's.
        std::fs::create_dir_all(root.join("reports/config")).unwrap();
        std::fs::write(
            root.join("reports/config/test.toml"),
            "[database]\nurl = \"postgres://u:p@localhost/x_reports_test\"\n",
        )
        .unwrap();
        assert_eq!(
            test_config_path(&root, "reports"),
            root.join("reports/config/test.toml")
        );
        assert_eq!(
            test_config_path(&root, "e2e"),
            root.join("api/config/test.toml")
        );
        assert_eq!(
            test_database_url(&test_config_path(&root, "reports")).as_deref(),
            Some("postgres://u:p@localhost/x_reports_test")
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
