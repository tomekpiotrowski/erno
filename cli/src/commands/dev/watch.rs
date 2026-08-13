use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use super::process::Supervisor;
use super::{CYAN, RESET};

const DEBOUNCE: Duration = Duration::from_millis(400);

/// Watch `api/` (except `target/`) and restart the API process on source changes.
pub fn spawn_api_watcher(
    api_dir: PathBuf,
    api: Supervisor,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = wait_for_change(&api_dir) => {
                    eprintln!("{CYAN}[api]{RESET} source changed — rebuilding");
                    api.restart().await;
                }
                _ = wait_shutdown(&mut shutdown) => break,
            }
        }
    });
}

async fn wait_shutdown(rx: &mut tokio::sync::watch::Receiver<bool>) {
    while !*rx.borrow_and_update() {
        if rx.changed().await.is_err() {
            break;
        }
    }
}

async fn wait_for_change(api_dir: &Path) {
    let dir = api_dir.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || watch_blocking(&dir)).await;
}

fn watch_blocking(api_dir: &Path) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            eprintln!(
                "{CYAN}[api]{RESET} file watcher unavailable ({e}) — API will not auto-reload"
            );
            // Block forever so we don't spin.
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
    };

    if let Err(e) = watcher.watch(api_dir, RecursiveMode::Recursive) {
        eprintln!("{CYAN}[api]{RESET} cannot watch {}: {e}", api_dir.display());
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    loop {
        match rx.recv() {
            Ok(Ok(event)) if event_is_relevant(api_dir, &event) => {
                // Debounce a burst of writes from rustfmt / cargo.
                let deadline = std::time::Instant::now() + DEBOUNCE;
                while let Ok(Ok(extra)) =
                    rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                {
                    let _ = extra;
                }
                return true;
            }
            Ok(Ok(_)) => {}
            Ok(Err(_)) | Err(_) => return false,
        }
    }
}

pub fn event_is_relevant(api_dir: &Path, event: &Event) -> bool {
    event.paths.iter().any(|p| is_relevant_path(api_dir, p))
}

pub fn is_relevant_path(api_dir: &Path, path: &Path) -> bool {
    if path.components().any(|c| c.as_os_str() == "target") {
        return false;
    }
    if !path.starts_with(api_dir) {
        return false;
    }
    if path.extension().and_then(|e| e.to_str()) == Some("rs") {
        return true;
    }
    if path.extension().and_then(|e| e.to_str()) == Some("toml") {
        return true;
    }
    path.file_name()
        .is_some_and(|n| n == "Cargo.lock" || n == "build.rs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_target_and_unrelated_files() {
        let api = Path::new("/proj/api");
        assert!(is_relevant_path(api, Path::new("/proj/api/src/main.rs")));
        assert!(is_relevant_path(api, Path::new("/proj/api/Cargo.toml")));
        assert!(is_relevant_path(
            api,
            Path::new("/proj/api/config/development.toml")
        ));
        assert!(!is_relevant_path(
            api,
            Path::new("/proj/api/target/debug/foo")
        ));
        assert!(!is_relevant_path(api, Path::new("/proj/api/README.md")));
        assert!(!is_relevant_path(api, Path::new("/proj/app/src/main.ts")));
    }
}
