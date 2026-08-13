mod banner;
mod log;
mod preflight;
mod process;
mod project;
mod selection;

use std::sync::Arc;

use clap::Args;
use tokio::process::Command;

use banner::{print_banner, spawn_readiness_watcher, starting_snapshot, DevUrls};
use log::LogSink;
use process::{spawn_labeled, Supervisor};
use project::resolve_project_root;

#[derive(Args, Default, Clone, Debug)]
pub struct DevArgs {
    /// Print every child log line instead of errors and ready events only
    #[arg(long, short = 'v')]
    pub verbose: bool,
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
}

pub(crate) const CYAN: &str = "\x1b[36m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const MAGENTA: &str = "\x1b[35m";
pub(crate) const YELLOW: &str = "\x1b[33m";
pub(crate) const DIM: &str = "\x1b[2m";
pub(crate) const RESET: &str = "\x1b[0m";

pub async fn handle_dev(root: Option<std::path::PathBuf>, args: DevArgs) {
    let root = resolve_project_root(root);
    let api_dir = root.join("api");
    let app_dir = root.join("app");
    let www_dir = root.join("www");

    let has_www = www_dir.is_dir() && www_dir.join("package.json").is_file();
    let sel = match selection::ServiceSelection::resolve(&args, has_www) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    };

    if sel.app && !app_dir.is_dir() {
        eprintln!("Found project at {} but no app/ directory.", root.display());
        eprintln!("Pass --api to start only the API, or scaffold an app with `erno new`.");
        std::process::exit(1);
    }

    let mut ports = Vec::new();
    if sel.api {
        ports.push(preflight::DEFAULT_API_PORT);
    }
    if sel.app {
        ports.push(preflight::DEFAULT_APP_PORT);
    }
    if sel.www {
        ports.push(preflight::DEFAULT_WWW_PORT);
    }
    preflight::run_preflight(sel.api, &ports);

    if sel.app {
        ensure_npm_deps(&app_dir, "app");
    }
    if sel.www {
        ensure_npm_deps(&www_dir, "www");
    }

    let sink = Arc::new(LogSink::new(&root, args.verbose));
    if !args.verbose {
        if let Some(path) = sink.path() {
            println!(
                "{DIM}Logs → {}  (use --verbose for the live multiplex){RESET}",
                path.display()
            );
        }
    }

    let urls = DevUrls::defaults(sel.api, sel.app, sel.www);
    print_banner(&urls, &starting_snapshot(&urls));
    spawn_readiness_watcher(urls);

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let api = sel.api.then(|| {
        let use_watch = has_cargo_watch();
        if !use_watch {
            println!(
                "{CYAN}[api]{RESET} cargo-watch not found — run `cargo install cargo-watch` for auto-reload"
            );
        }
        let api_dir_spawn = api_dir.clone();
        let api_sink = sink.clone();
        Supervisor::start("api", CYAN, shutdown_rx.clone(), move || {
            let mut cmd = Command::new("cargo");
            if use_watch {
                cmd.args(["watch", "-x", "run"]);
            } else {
                cmd.arg("run");
            }
            spawn_labeled(cmd, &api_dir_spawn, CYAN, "api", api_sink.clone())
        })
    });

    let app = sel.app.then(|| {
        let app_dir_spawn = app_dir.clone();
        let app_sink = sink.clone();
        Supervisor::start("app", GREEN, shutdown_rx.clone(), move || {
            let mut cmd = Command::new("npm");
            cmd.arg("start");
            spawn_labeled(cmd, &app_dir_spawn, GREEN, "app", app_sink.clone())
        })
    });

    let www = sel.www.then(|| {
        let www_dir_spawn = www_dir.clone();
        let www_sink = sink.clone();
        Supervisor::start("www", MAGENTA, shutdown_rx.clone(), move || {
            let mut cmd = Command::new("npm");
            cmd.args(["run", "dev"]);
            spawn_labeled(cmd, &www_dir_spawn, MAGENTA, "www", www_sink.clone())
        })
    });

    let _ = tokio::signal::ctrl_c().await;
    eprintln!("\nShutting down...");
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
}

fn ensure_npm_deps(dir: &std::path::Path, label: &str) {
    if dir.join("node_modules").exists() {
        return;
    }
    println!("Installing {label} npm dependencies...");
    let status = std::process::Command::new("npm")
        .arg("install")
        .current_dir(dir)
        .status();
    match status {
        Err(e) => {
            eprintln!("Failed to run npm install in {label}/: {e}");
            std::process::exit(1);
        }
        Ok(s) if !s.success() => {
            eprintln!("npm install failed in {label}/.");
            std::process::exit(1);
        }
        _ => {}
    }
}

fn has_cargo_watch() -> bool {
    std::process::Command::new("cargo")
        .args(["watch", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
