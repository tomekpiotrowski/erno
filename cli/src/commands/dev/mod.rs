mod banner;
mod device;
mod lock;
mod log;
mod mail;
mod open;
mod ports;
mod preflight;
mod process;
mod project;
mod prometheus;
mod seed;
mod selection;
mod watch;

use std::sync::Arc;

use clap::Args;
use tokio::process::Command;

use banner::{print_banner, spawn_readiness_watcher, starting_snapshot};
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
    /// Skip Prometheus (otherwise required when starting the API)
    #[arg(long)]
    pub no_prometheus: bool,
    /// Do not start the operator admin SPA
    #[arg(long)]
    pub no_admin: bool,
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

    let mut urls = ports::discover_urls(&root, &sel);
    if sel.api && !args.no_prometheus {
        urls.prometheus = Some(prometheus::LISTEN_URL.to_string());
    }

    let device = if args.ios {
        Some(device::DevicePlatform::Ios)
    } else if args.android {
        Some(device::DevicePlatform::Android)
    } else {
        None
    };

    let mut _url_rewrite = None;
    let mut cors_env = None;
    if let Some(platform) = device {
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
            "Device live-reload ({}) on {ip}",
            platform.as_str()
        ));
    }

    preflight::run_preflight(
        sel.api,
        sel.api && !args.no_prometheus,
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

    print_banner(&urls, &starting_snapshot(&urls));
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
    spawn_readiness_watcher(urls.clone());

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
        Supervisor::start("app", shutdown_rx.clone(), move || {
            let cmd = if let Some(platform) = device {
                let mut cmd = Command::new("npx");
                cmd.args([
                    "ionic",
                    "cap",
                    "run",
                    platform.as_str(),
                    "--livereload",
                    "--external",
                    "--port",
                    &app_port.to_string(),
                ]);
                cmd
            } else {
                let mut cmd = Command::new("npm");
                cmd.arg("start");
                cmd
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

    let admin_dir = find_admin_dir(&root);
    let admin = if !args.no_admin {
        if let Some(admin_dir) = admin_dir {
            ensure_npm_deps(&admin_dir, "admin")?;
            let admin_sink = sink.clone();
            Some(Supervisor::start("admin", shutdown_rx.clone(), move || {
                let mut cmd = Command::new("npm");
                cmd.arg("start");
                spawn_labeled(cmd, &admin_dir, "admin", admin_sink.clone())
            }))
        } else {
            None
        }
    } else {
        None
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

    let _ = tokio::signal::ctrl_c().await;
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
    if let Some(admin) = admin {
        admin.shutdown().await;
    }
    Ok(())
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
