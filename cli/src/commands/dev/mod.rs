mod banner;
mod preflight;
mod process;
mod project;

use std::sync::Arc;

use tokio::process::Command;
use tokio::sync::Mutex;

use banner::{print_banner, spawn_readiness_watcher, starting_snapshot, DevUrls};
use process::{kill_child, spawn_labeled, wait_child};
use project::resolve_project_root;

pub(crate) const CYAN: &str = "\x1b[36m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const MAGENTA: &str = "\x1b[35m";
pub(crate) const YELLOW: &str = "\x1b[33m";
pub(crate) const DIM: &str = "\x1b[2m";
pub(crate) const RESET: &str = "\x1b[0m";

pub async fn handle_dev(root: Option<std::path::PathBuf>) {
    let root = resolve_project_root(root);
    let api_dir = root.join("api");
    let app_dir = root.join("app");
    let www_dir = root.join("www");

    if !app_dir.is_dir() {
        eprintln!("Found project at {} but no app/ directory.", root.display());
        eprintln!("Run `erno dev` from a full-stack Erno project (api/ + app/).");
        std::process::exit(1);
    }

    let has_www = www_dir.is_dir() && www_dir.join("package.json").is_file();

    preflight::run_preflight(has_www);

    ensure_npm_deps(&app_dir, "app");
    if has_www {
        ensure_npm_deps(&www_dir, "www");
    }

    let urls = DevUrls::defaults(has_www);
    print_banner(&urls, &starting_snapshot(&urls));
    spawn_readiness_watcher(urls);

    let api_cmd = if has_cargo_watch() {
        let mut cmd = Command::new("cargo");
        cmd.args(["watch", "-x", "run"]);
        cmd
    } else {
        println!(
            "{CYAN}[api]{RESET} cargo-watch not found — run `cargo install cargo-watch` for auto-reload"
        );
        let mut cmd = Command::new("cargo");
        cmd.arg("run");
        cmd
    };

    let api_child = Arc::new(Mutex::new(spawn_labeled(api_cmd, &api_dir, CYAN, "api")));

    let mut app_cmd = Command::new("npm");
    app_cmd.arg("start");
    let app_child = Arc::new(Mutex::new(spawn_labeled(app_cmd, &app_dir, GREEN, "app")));

    let www_child = if has_www {
        let mut www_cmd = Command::new("npm");
        www_cmd.args(["run", "dev"]);
        Some(Arc::new(Mutex::new(spawn_labeled(
            www_cmd, &www_dir, MAGENTA, "www",
        ))))
    } else {
        None
    };

    let api_handle = api_child.clone();
    let app_handle = app_child.clone();

    if let Some(www) = www_child {
        tokio::select! {
            _ = wait_child(api_child.clone()) => {
                eprintln!("\n{CYAN}[api]{RESET} process exited — shutting down.");
                kill_child(&app_handle).await;
                kill_child(&www).await;
            }
            _ = wait_child(app_child.clone()) => {
                eprintln!("\n{GREEN}[app]{RESET} process exited — shutting down.");
                kill_child(&api_handle).await;
                kill_child(&www).await;
            }
            _ = wait_child(www.clone()) => {
                eprintln!("\n{MAGENTA}[www]{RESET} process exited — shutting down.");
                kill_child(&api_handle).await;
                kill_child(&app_handle).await;
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nShutting down...");
                kill_child(&api_handle).await;
                kill_child(&app_handle).await;
                kill_child(&www).await;
            }
        }
    } else {
        tokio::select! {
            _ = wait_child(api_child.clone()) => {
                eprintln!("\n{CYAN}[api]{RESET} process exited — shutting down.");
                kill_child(&app_handle).await;
            }
            _ = wait_child(app_child.clone()) => {
                eprintln!("\n{GREEN}[app]{RESET} process exited — shutting down.");
                kill_child(&api_handle).await;
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nShutting down...");
                kill_child(&api_handle).await;
                kill_child(&app_handle).await;
            }
        }
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
