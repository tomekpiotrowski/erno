use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

pub async fn handle_dev(root: Option<std::path::PathBuf>) {
    let root =
        root.unwrap_or_else(|| std::env::current_dir().expect("cannot read current directory"));
    let api_dir = root.join("api");
    let app_dir = root.join("app");

    if !api_dir.is_dir() {
        eprintln!("No api/ directory found. Run `erno dev` from your project root.");
        std::process::exit(1);
    }
    if !app_dir.is_dir() {
        eprintln!("No app/ directory found. Run `erno dev` from your project root.");
        std::process::exit(1);
    }

    if !app_dir.join("node_modules").exists() {
        println!("Installing npm dependencies...");
        let status = std::process::Command::new("npm")
            .arg("install")
            .current_dir(&app_dir)
            .status();
        match status {
            Err(e) => {
                eprintln!("Failed to run npm install: {e}");
                std::process::exit(1);
            }
            Ok(s) if !s.success() => {
                eprintln!("npm install failed.");
                std::process::exit(1);
            }
            _ => {}
        }
    }

    let mut api_cmd = if has_cargo_watch() {
        let mut cmd = Command::new("cargo");
        cmd.args(["watch", "-x", "run"]);
        cmd
    } else {
        println!("{CYAN}[api]{RESET} cargo-watch not found — run `cargo install cargo-watch` for auto-reload");
        let mut cmd = Command::new("cargo");
        cmd.arg("run");
        cmd
    };

    #[cfg(unix)]
    api_cmd.process_group(0);

    let mut api_child = api_cmd
        .current_dir(&api_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn api process");

    let mut app_cmd = Command::new("npm");
    app_cmd.arg("start");
    #[cfg(unix)]
    app_cmd.process_group(0);

    let mut app_child = app_cmd
        .current_dir(&app_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn `npm start`");

    let api_stdout = BufReader::new(api_child.stdout.take().unwrap());
    let api_stderr = BufReader::new(api_child.stderr.take().unwrap());
    let app_stdout = BufReader::new(app_child.stdout.take().unwrap());
    let app_stderr = BufReader::new(app_child.stderr.take().unwrap());

    spawn_printer(api_stdout, CYAN, "api");
    spawn_printer(api_stderr, CYAN, "api");
    spawn_printer(app_stdout, GREEN, "app");
    spawn_printer(app_stderr, GREEN, "app");

    let api_child = Arc::new(Mutex::new(api_child));
    let app_child = Arc::new(Mutex::new(app_child));

    let api_handle = api_child.clone();
    let app_handle = app_child.clone();

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

fn spawn_printer<R>(reader: R, color: &'static str, label: &'static str)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            println!("{color}[{label}]{RESET} {line}");
        }
    });
}

async fn wait_child(child: Arc<Mutex<Child>>) {
    let _ = child.lock().await.wait().await;
}

async fn kill_child(child: &Arc<Mutex<Child>>) {
    let mut guard = child.lock().await;

    // Kill the entire process group so grandchildren (e.g. cargo run, ng serve)
    // don't survive after their parent (cargo watch, npm) is gone.
    #[cfg(unix)]
    if let Some(pid) = guard.id() {
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    }

    let _ = guard.kill().await;
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
