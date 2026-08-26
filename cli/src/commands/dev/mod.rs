mod banner;
mod device;
mod lock;
mod log;
mod loki;
mod mail;
mod open;
mod ports;
mod preflight;
mod process;
mod project;
mod prometheus;
mod seed;
mod selection;
mod tempo;
mod tui;
mod watch;

use std::sync::Arc;

use clap::Args;
use tokio::process::Command;

use banner::spawn_readiness_watcher;
use log::LogSink;
use process::{spawn_labeled, Supervisor};
pub use project::resolve_project_root;

use crate::ui;

#[derive(Args, Default, Clone, Debug)]
pub struct DevArgs {
    /// Start only the API (can be combined with --app / --www)
    #[arg(long)]
    pub api: bool,
    /// Start only the product app (can be combined with --api / --www)
    #[arg(long)]
    pub app: bool,
    /// Start only the marketing site (can be combined with --api / --app)
    #[arg(long)]
    pub www: bool,
    /// Skip the marketing site even when www/ is present
    #[arg(long)]
    pub no_www: bool,
    /// Ensure a verified demo user exists (dev@example.com / password)
    #[arg(long)]
    pub seed: bool,
    /// Open the marketing site (or app, or API) in a browser once it is ready
    #[arg(long)]
    pub open: bool,
    /// Live-reload the app on a connected iOS device / simulator
    #[arg(long)]
    pub ios: bool,
    /// Live-reload the app on a connected Android device / emulator
    #[arg(long)]
    pub android: bool,
    /// Device / emulator id for --ios or --android (only needed when several are attached)
    #[arg(long, value_name = "ID")]
    pub target: Option<String>,
    /// Skip Prometheus (otherwise required when starting the API)
    #[arg(long)]
    pub no_prometheus: bool,
    /// Skip Tempo (otherwise required when starting the API)
    #[arg(long)]
    pub no_tempo: bool,
    /// Skip Loki (otherwise required when starting the API)
    #[arg(long)]
    pub no_loki: bool,
    /// Do not start the operator admin SPA
    #[arg(long)]
    pub no_admin: bool,
    /// Skip the monitoring collector and console even when monitoring/ is present
    #[arg(long)]
    pub no_monitoring: bool,
    /// Extra `[[package.dev]]` service to start (repeatable; added to the usual stack)
    #[arg(long)]
    pub package: Vec<String>,
    /// Start every `[[package.dev]]`, including `default = false`
    #[arg(long)]
    pub all: bool,
    /// Keep the pinned banner instead of the interactive dashboard
    #[arg(long)]
    pub no_ui: bool,
}

pub async fn handle_dev(root: Option<std::path::PathBuf>, args: DevArgs) -> ui::Cmd {
    let root = resolve_project_root(root)?;
    // Held for the lifetime of the command; its `Drop` removes .erno/dev.lock,
    // which is why everything below returns rather than calling `exit`.
    let _lock = lock::DevLock::acquire(&root)?;
    let api_dir = root.join("api");
    let app_dir = root.join("app");
    let www_dir = root.join("www");

    let has_www = www_dir.is_dir() && www_dir.join("package.json").is_file();
    let sel = selection::ServiceSelection::resolve(&args, has_www)?;

    if args.seed && !sel.api {
        return Err("--seed requires the API (pass --api or omit service flags).".into());
    }

    if sel.app && !app_dir.is_dir() {
        return Err(format!(
            "found a project at {} but no app/ directory\n\
             Pass --api to start only the API, or scaffold an app with `erno new`.",
            root.display()
        )
        .into());
    }

    let packages = crate::commands::packages::load_packages(&root)?;
    let extras = selection::extra_services(&packages, &args.package, args.all)?;

    // Resolved here rather than at the spawn below, because the banner is
    // pinned long before the admin child starts and its row has to be there
    // from the first frame — the pinned region's height cannot change later.
    let admin_dir = (!args.no_admin).then(|| find_admin_dir(&root)).flatten();
    // Gated on `sel.api`: the collector exists to receive the app's reports, so
    // starting it for `erno dev --www` would be noise.
    let monitoring_dir = (sel.api && !args.no_monitoring)
        .then(|| find_monitoring_dir(&root))
        .flatten();
    let console_dir = monitoring_dir
        .as_ref()
        .map(|d| d.join("ui"))
        .filter(|d| d.join("package.json").is_file());

    let mut urls = ports::discover_urls(&root, &sel);
    if sel.api && !args.no_prometheus {
        urls.prometheus = Some(prometheus::LISTEN_URL.to_string());
    }
    if sel.api && !args.no_tempo {
        urls.tempo = Some(tempo::LISTEN_URL.to_string());
    }
    if sel.api && !args.no_loki {
        urls.loki = Some(loki::LISTEN_URL.to_string());
    }
    urls.extra = extras
        .iter()
        .map(|e| (e.name.clone(), e.url.clone()))
        .collect();
    if admin_dir.is_some() {
        urls.admin = Some(ADMIN_URL.to_string());
    }
    if monitoring_dir.is_some() {
        urls.monitoring = Some(monitoring_url());
    }
    if let Some(dir) = &console_dir {
        // The console's port lives in package.json's start script, not in
        // angular.json — `read_angular_port` returns None for it.
        urls.console = Some(ports::port_from_package_script(dir, "start").map_or_else(
            || CONSOLE_URL.to_string(),
            |p| format!("http://localhost:{p}"),
        ));
    }

    let device = if args.ios {
        Some(device::DevicePlatform::Ios)
    } else if args.android {
        Some(device::DevicePlatform::Android)
    } else {
        None
    };

    if args.target.is_some() && device.is_none() {
        return Err("--target requires --ios or --android.".into());
    }

    let mut _url_rewrite = None;
    let mut cors_env = None;
    let mut ionic = None;
    let mut device_target = None;
    if let Some(platform) = device {
        // Everything that can fail cheaply runs before `apply_lan_api_urls`,
        // which edits a file in app/ and only restores it on drop.
        device::ensure_platform_added(&app_dir, platform)?;
        let cli = device::resolve_ionic(&app_dir);
        if cli.is_npx() {
            ui::warn("Ionic CLI not found in app/node_modules or on PATH");
            ui::detail(
                "Fetching it with npx for this run.\n\
                 Install it once with `npm install --save-dev @ionic/cli` in app/.",
            );
        }
        let target = match args.target.clone() {
            Some(target) => target,
            None => device::choose_target(&device::list_targets(&app_dir, platform)?, platform)?,
        };
        ionic = Some(cli);
        device_target = Some(target);

        let ip = device::lan_ip().ok_or_else(|| {
            format!(
                "could not detect a LAN IP for `--{}`\n\
                 Connect to a network and try again.",
                platform.as_str()
            )
        })?;
        if let Some(api) = urls.api.clone() {
            let api_http = device::rewrite_url_host(&api, ip);
            let api_ws = if api_http.starts_with("https://") {
                api_http.replacen("https://", "wss://", 1)
            } else {
                api_http.replacen("http://", "ws://", 1)
            };
            urls.api = Some(api_http.clone());
            if let Some(app) = urls.app.clone() {
                urls.app = Some(device::rewrite_url_host(&app, ip));
            }
            _url_rewrite = Some(device::apply_lan_api_urls(&app_dir, &api_http, &api_ws)?);
            if let Some(app) = &urls.app {
                cors_env = Some(device::cors_origins(app));
            }
        }
        ui::info(format!(
            "Device live-reload ({}) on {ip} → {}",
            platform.as_str(),
            device_target.as_deref().unwrap_or("?"),
        ));
    }

    preflight::run_preflight(
        sel.api,
        sel.api && !args.no_prometheus,
        sel.api && !args.no_tempo,
        sel.api && !args.no_loki,
        &ports::ports_to_check(&urls),
    )?;

    if sel.app {
        ensure_npm_deps(&app_dir, "app")?;
    }
    if sel.www {
        ensure_npm_deps(&www_dir, "www")?;
    }

    let sink = Arc::new(LogSink::new(&root));
    if !ui::verbose() {
        if let Some(path) = sink.path() {
            ui::info(format!(
                "Logs → {}  (use --verbose for the live multiplex)",
                path.display()
            ));
        }
    }

    let use_tui = tui::TuiGate::from_env(args.no_ui).should_start();
    if use_tui {
        sink.set_capture_only(true);
    }

    // Held for the lifetime of the command like `_lock` above: its `Drop` takes
    // the pinned banner off the screen, which is why the `?`s below are safe.
    // `None` means this terminal cannot pin, so the banner scrolled and the
    // readiness watcher narrates each change as a row instead. The TUI path
    // skips the banner entirely.
    let banner = if use_tui { None } else { banner::start(&urls) };
    let sticky = banner.is_some();
    if args.open {
        if let Some(url) = open::url_to_open(
            urls.www.as_deref(),
            urls.app.as_deref(),
            urls.api.as_deref(),
        ) {
            open::spawn_opener(url);
        }
    }

    if let Some(api_url) = urls.api.clone() {
        mail::spawn_mail_watcher(api_url.clone());
        let seed_root = root.clone();
        let force_seed = args.seed;
        tokio::spawn(async move {
            seed::maybe_seed(&seed_root, &api_url, force_seed).await;
        });
    }
    if !use_tui {
        spawn_readiness_watcher(urls.clone(), sticky);
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let api = sel.api.then(|| {
        let api_dir_spawn = api_dir.clone();
        let api_sink = sink.clone();
        let cors_env = cors_env.clone();
        Supervisor::start("api", shutdown_rx.clone(), move || {
            let mut cmd = Command::new("cargo");
            cmd.arg("run");
            if let Some(origins) = cors_env.as_deref() {
                cmd.env("ERNO_DEV_CORS_ORIGINS", origins);
            }
            spawn_labeled(cmd, &api_dir_spawn, "api", api_sink.clone())
        })
    });
    if let Some(api) = api.as_ref() {
        watch::spawn_api_watcher(api_dir.clone(), api.clone(), shutdown_rx.clone());
    }

    let app_port = urls
        .app
        .as_deref()
        .and_then(|u| ports::port_from_url(Some(u)))
        .unwrap_or(4200);
    let app = sel.app.then(|| {
        let app_dir_spawn = app_dir.clone();
        let app_sink = sink.clone();
        let ionic = ionic.clone();
        let device_target = device_target.clone();
        Supervisor::start("app", shutdown_rx.clone(), move || {
            let cmd = match (device, ionic.as_ref(), device_target.as_deref()) {
                (Some(platform), Some(ionic), Some(target)) => {
                    let mut cmd = Command::new(&ionic.program);
                    cmd.args(ionic.cap_run_args(platform, app_port, target));
                    cmd
                }
                _ => {
                    let mut cmd = Command::new("npm");
                    cmd.arg("start");
                    cmd
                }
            };
            spawn_labeled(cmd, &app_dir_spawn, "app", app_sink.clone())
        })
    });

    let api_port = urls
        .api
        .as_deref()
        .and_then(|u| ports::port_from_url(Some(u)))
        .unwrap_or(3000);
    // `run_preflight` above already exited if the binary was missing under this
    // exact condition, so there is no second check here.
    let prometheus = if sel.api && !args.no_prometheus {
        let metrics_toml =
            std::fs::read_to_string(root.join("api/config/development.toml")).unwrap_or_default();
        let scrape_token = ports::parse_table_string(&metrics_toml, "metrics", "auth_token");
        let dir = prometheus::prepare_dir(&root, api_port, scrape_token.as_deref())
            .map_err(|e| format!("could not prepare the Prometheus data dir: {e}"))?;
        let prom_sink = sink.clone();
        Some(Supervisor::start("prom", shutdown_rx.clone(), move || {
            prometheus::spawn(&dir, prom_sink.clone())
        }))
    } else {
        None
    };

    let tempo = if sel.api && !args.no_tempo {
        let dir = tempo::prepare_dir(&root)
            .map_err(|e| format!("could not prepare the Tempo data dir: {e}"))?;
        let sink = sink.clone();
        Some(Supervisor::start("tempo", shutdown_rx.clone(), move || {
            tempo::spawn(&dir, sink.clone())
        }))
    } else {
        None
    };

    let loki = if sel.api && !args.no_loki {
        let dir = loki::prepare_dir(&root)
            .map_err(|e| format!("could not prepare the Loki data dir: {e}"))?;
        let sink = sink.clone();
        Some(Supervisor::start("loki", shutdown_rx.clone(), move || {
            loki::spawn(&dir, sink.clone())
        }))
    } else {
        None
    };

    let admin = match admin_dir {
        Some(admin_dir) => {
            ensure_npm_deps(&admin_dir, "admin")?;
            let admin_sink = sink.clone();
            Some(Supervisor::start("admin", shutdown_rx.clone(), move || {
                let mut cmd = Command::new("npm");
                cmd.arg("start");
                spawn_labeled(cmd, &admin_dir, "admin", admin_sink.clone())
            }))
        }
        None => None,
    };

    // The collector migrates its own database on boot, exactly like the api, so
    // there is no separate `db migrate up` step here. The database itself must
    // already exist; `erno dev` warns rather than failing if it does not.
    let monitoring = match monitoring_dir {
        Some(monitoring_dir) => {
            let mon_sink = sink.clone();
            Some(Supervisor::start("mon", shutdown_rx.clone(), move || {
                let mut cmd = Command::new("cargo");
                cmd.arg("run");
                spawn_labeled(cmd, &monitoring_dir, "mon", mon_sink.clone())
            }))
        }
        None => None,
    };

    let console = match console_dir {
        Some(console_dir) => {
            ensure_npm_deps(&console_dir, "console")?;
            let console_sink = sink.clone();
            Some(Supervisor::start(
                "console",
                shutdown_rx.clone(),
                move || {
                    let mut cmd = Command::new("npm");
                    cmd.arg("start");
                    spawn_labeled(cmd, &console_dir, "console", console_sink.clone())
                },
            ))
        }
        None => None,
    };

    let www = sel.www.then(|| {
        let www_dir_spawn = www_dir.clone();
        let www_sink = sink.clone();
        Supervisor::start("www", shutdown_rx.clone(), move || {
            let mut cmd = Command::new("npm");
            cmd.args(["run", "dev"]);
            spawn_labeled(cmd, &www_dir_spawn, "www", www_sink.clone())
        })
    });

    let extra_supervisors: Vec<Supervisor> = extras
        .into_iter()
        .map(|svc| {
            let dir = root.join(svc.dir);
            let extra_sink = sink.clone();
            Supervisor::start(svc.name.clone(), shutdown_rx.clone(), move || {
                let mut cmd = Command::new(&svc.command);
                cmd.args(&svc.args);
                spawn_labeled(cmd, &dir, svc.name.clone(), extra_sink.clone())
            })
        })
        .collect();

    if use_tui {
        let mut supervisors = std::collections::HashMap::new();
        if let Some(s) = api.clone() {
            supervisors.insert("api".into(), s);
        }
        if let Some(s) = app.clone() {
            supervisors.insert("app".into(), s);
        }
        if let Some(s) = www.clone() {
            supervisors.insert("www".into(), s);
        }
        if let Some(s) = prometheus.clone() {
            supervisors.insert("prom".into(), s);
        }
        if let Some(s) = tempo.clone() {
            supervisors.insert("tempo".into(), s);
        }
        if let Some(s) = loki.clone() {
            supervisors.insert("loki".into(), s);
        }
        if let Some(s) = admin.clone() {
            supervisors.insert("admin".into(), s);
        }
        if let Some(s) = console.clone() {
            supervisors.insert("console".into(), s);
        }
        if let Some(s) = monitoring.clone() {
            supervisors.insert("mon".into(), s);
        }
        for (i, s) in extra_supervisors.iter().enumerate() {
            if let Some((name, _)) = urls.extra.get(i) {
                supervisors.insert(name.clone(), s.clone());
            }
        }
        let project = root.file_name().and_then(|s| s.to_str()).unwrap_or("erno");
        let metrics_toml =
            std::fs::read_to_string(root.join("api/config/development.toml")).unwrap_or_default();
        let scrape_token = ports::parse_table_string(&metrics_toml, "metrics", "auth_token");
        let opts = tui::TuiOpts {
            prometheus: urls.prometheus.clone(),
            prometheus_token: scrape_token,
            tempo: urls.tempo.clone(),
            loki: urls.loki.clone(),
            api: urls.api.clone(),
            console: urls.console.clone(),
        };
        if let Err(e) = tui::run(&urls, sink.clone(), project, supervisors, opts).await {
            ui::warn(e);
        }
    } else {
        let _ = tokio::signal::ctrl_c().await;
    }
    // Unpin before anything else prints: the final banner lands in the
    // scrollback, and the watcher stops narrating as the children go down.
    drop(banner);
    ui::blank();
    ui::info("Shutting down...");
    let _ = shutdown_tx.send(true);
    if let Some(api) = api {
        api.shutdown().await;
    }
    if let Some(app) = app {
        app.shutdown().await;
    }
    if let Some(www) = www {
        www.shutdown().await;
    }
    if let Some(prometheus) = prometheus {
        prometheus.shutdown().await;
    }
    if let Some(tempo) = tempo {
        tempo.shutdown().await;
    }
    if let Some(loki) = loki {
        loki.shutdown().await;
    }
    if let Some(admin) = admin {
        admin.shutdown().await;
    }
    if let Some(console) = console {
        console.shutdown().await;
    }
    if let Some(monitoring) = monitoring {
        monitoring.shutdown().await;
    }
    for extra in extra_supervisors {
        extra.shutdown().await;
    }
    Ok(())
}

/// Where the operator admin SPA serves from. `admin/package.json` starts
/// `ng serve --port 4300`, and the two have to agree.
pub const ADMIN_URL: &str = "http://localhost:4300";

/// Where the monitoring operator console serves from when its start script
/// cannot be read. `monitoring/ui/package.json` starts `ng serve --port 4400`.
pub const CONSOLE_URL: &str = "http://localhost:4400";

/// Where the collector listens. Derived from the port Prometheus is configured
/// to scrape, so the two cannot drift apart.
pub fn monitoring_url() -> String {
    format!("http://localhost:{}", prometheus::MONITORING_PORT)
}

/// The monitoring deployment, local to this project only.
///
/// Deliberately **not** mirroring `find_admin_dir`'s sibling fallback. The admin
/// SPA is a pure client that proxies to whichever API is running, so borrowing
/// the framework's copy is harmless. The collector is not: it has its own
/// database and its own config, so falling back to the framework's `monitoring/`
/// would run erno's own collector against `erno_monitoring` while you develop a
/// different project — silently mixing two projects' error data.
fn find_monitoring_dir(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let local = root.join("monitoring");
    local.join("Cargo.toml").is_file().then_some(local)
}

fn find_admin_dir(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let local = root.join("admin");
    if local.join("package.json").is_file() {
        return Some(local);
    }
    let cargo = std::fs::read_to_string(root.join("api/Cargo.toml")).ok()?;
    for line in cargo.lines() {
        let line = line.trim();
        if !line.starts_with("erno") || !line.contains("path") {
            continue;
        }
        let path = line.split("path").nth(1)?;
        let path = path.split('"').nth(1).or_else(|| path.split('\'').nth(1))?;
        let api_dir = root.join("api").join(path);
        let admin = api_dir.parent()?.join("admin");
        if admin.join("package.json").is_file() {
            return Some(admin);
        }
    }
    None
}

fn ensure_npm_deps(dir: &std::path::Path, label: &str) -> Result<(), String> {
    if dir.join("node_modules").exists() {
        return Ok(());
    }
    ui::info(format!("Installing {label} npm dependencies..."));
    let status = std::process::Command::new("npm")
        .arg("install")
        .current_dir(dir)
        .status();
    match status {
        Err(e) => Err(format!("could not run npm install in {label}/: {e}")),
        Ok(s) if !s.success() => Err(format!("npm install failed in {label}/")),
        _ => Ok(()),
    }
}
