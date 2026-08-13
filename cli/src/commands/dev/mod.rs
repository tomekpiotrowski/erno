mod banner;
mod lock;
mod log;
mod mail;
mod ports;
mod preflight;
mod process;
mod project;
mod seed;
mod selection;
mod watch;

use std::sync::Arc;

use clap::Args;
use tokio::process::Command;

use banner::{print_banner, spawn_readiness_watcher, starting_snapshot};
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
    /// Ensure a verified demo user exists (dev@example.com / password)
    #[arg(long)]
    pub seed: bool,
}

pub(crate) const CYAN: &str = "\x1b[36m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const MAGENTA: &str = "\x1b[35m";
pub(crate) const YELLOW: &str = "\x1b[33m";
pub(crate) const DIM: &str = "\x1b[2m";
pub(crate) const RESET: &str = "\x1b[0m";

pub async fn handle_dev(root: Option<std::path::PathBuf>, args: DevArgs) {
    let root = resolve_project_root(root);
    let _lock = match lock::DevLock::acquire(&root) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
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

    if args.seed && !sel.api {
        eprintln!("--seed requires the API (pass --api or omit service flags).");
        std::process::exit(1);
    }

    if sel.app && !app_dir.is_dir() {
        eprintln!("Found project at {} but no app/ directory.", root.display());
        eprintln!("Pass --api to start only the API, or scaffold an app with `erno new`.");
        std::process::exit(1);
    }

    let urls = ports::discover_urls(&root, &sel);
    preflight::run_preflight(sel.api, &ports::ports_to_check(&urls));

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

    print_banner(&urls, &starting_snapshot(&urls));
    if let Some(api_url) = urls.api.clone() {
        mail::spawn_mail_watcher(api_url.clone());
        let seed_root = root.clone();
        let force_seed = args.seed;
        tokio::spawn(async move {
            seed::maybe_seed(&seed_root, &api_url, force_seed).await;
        });
    }
    spawn_readiness_watcher(urls);

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let api = sel.api.then(|| {
        let api_dir_spawn = api_dir.clone();
        let api_sink = sink.clone();
        Supervisor::start("api", CYAN, shutdown_rx.clone(), move || {
            let mut cmd = Command::new("cargo");
            cmd.arg("run");
            spawn_labeled(cmd, &api_dir_spawn, CYAN, "api", api_sink.clone())
        })
    });
    if let Some(api) = api.as_ref() {
        watch::spawn_api_watcher(api_dir.clone(), api.clone(), shutdown_rx.clone());
    }

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
