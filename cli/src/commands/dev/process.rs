use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::RESET;

/// Force color and progress through piped stdout. Cargo, npm, and most CLIs
/// disable both when they detect a non-TTY (which is how we capture logs).
pub fn apply_child_color_env(cmd: &mut Command) {
    cmd.env("CARGO_TERM_COLOR", "always");
    cmd.env("CARGO_TERM_PROGRESS_WHEN", "always");
    cmd.env("CARGO_TERM_PROGRESS_WIDTH", "80");
    cmd.env("FORCE_COLOR", "1");
    cmd.env("CLICOLOR_FORCE", "1");
    cmd.env("npm_config_color", "always");
    if std::env::var_os("TERM").is_none() {
        cmd.env("TERM", "xterm-256color");
    }
}

pub fn spawn_labeled(
    mut cmd: Command,
    dir: &std::path::Path,
    color: &'static str,
    label: &'static str,
) -> Child {
    apply_child_color_env(&mut cmd);

    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!("failed to spawn {label} process: {e}");
            std::process::exit(1);
        });

    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stderr = BufReader::new(child.stderr.take().unwrap());
    spawn_printer(stdout, color, label);
    spawn_printer(stderr, color, label);
    child
}

pub fn spawn_printer<R>(reader: R, color: &'static str, label: &'static str)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            println!("{color}[{label}]{RESET} {line}");
        }
    });
}

pub async fn wait_child(child: Arc<Mutex<Child>>) {
    let _ = child.lock().await.wait().await;
}

pub async fn kill_child(child: &Arc<Mutex<Child>>) {
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
