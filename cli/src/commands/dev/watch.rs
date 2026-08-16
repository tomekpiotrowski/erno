use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use notify::event::{EventKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use super::process::Supervisor;
use crate::ui;

const DEBOUNCE: Duration = Duration::from_millis(400);
const SETTLE: Duration = Duration::from_secs(1);

/// Watch API sources and restart the API process on real content changes.
///
/// notify's inotify backend emits `OPEN`/`CLOSE_NOWRITE` (as `EventKind::Access`)
/// whenever rustc reads a `.rs` file. Treating those as edits kills `cargo run`
/// mid-compile and loops forever.
pub fn spawn_api_watcher(
    api_dir: PathBuf,
    api: Supervisor,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(
            move |res| {
                let _ = event_tx.send(res);
            },
            notify::Config::default(),
        ) {
            Ok(w) => w,
            Err(e) => {
                ui::prefixed(
                    ui::Stream::Err,
                    "api",
                    &format!("file watcher unavailable ({e}) — API will not auto-reload"),
                );
                return;
            }
        };

        if let Err(e) = watch_sources(&mut watcher, &api_dir) {
            ui::prefixed(
                ui::Stream::Err,
                "api",
                &format!("cannot watch {}: {e}", api_dir.display()),
            );
            return;
        }

        let (async_tx, mut async_rx) = tokio::sync::mpsc::unbounded_channel();
        std::thread::spawn(move || {
            while let Ok(ev) = event_rx.recv() {
                if async_tx.send(ev).is_err() {
                    break;
                }
            }
        });

        // Drop events generated while the watcher is attached and cargo first
        // opens every source file.
        let ignore_until = Instant::now() + SETTLE;

        loop {
            tokio::select! {
                maybe = async_rx.recv() => {
                    let Some(Ok(event)) = maybe else {
                        if maybe.is_none() {
                            break;
                        }
                        continue;
                    };
                    if Instant::now() < ignore_until {
                        continue;
                    }
                    if !event_is_relevant(&api_dir, &event) {
                        continue;
                    }
                    drain_until(&mut async_rx, Instant::now() + DEBOUNCE).await;
                    ui::prefixed(ui::Stream::Err, "api", "source changed — rebuilding");
                    api.restart().await;
                    // rustc will reopen every source file; ignore that burst.
                    drain_until(&mut async_rx, Instant::now() + SETTLE).await;
                }
                _ = wait_shutdown(&mut shutdown) => break,
            }
        }

        drop(watcher);
    });
}

fn watch_sources(watcher: &mut RecommendedWatcher, api_dir: &Path) -> notify::Result<()> {
    for name in ["src", "config", "migrations"] {
        let dir = api_dir.join(name);
        if dir.is_dir() {
            watcher.watch(&dir, RecursiveMode::Recursive)?;
        }
    }
    for name in ["Cargo.toml", "build.rs"] {
        let file = api_dir.join(name);
        if file.is_file() {
            watcher.watch(&file, RecursiveMode::NonRecursive)?;
        }
    }
    Ok(())
}

async fn drain_until(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<notify::Result<Event>>,
    until: Instant,
) {
    while Instant::now() < until {
        let remaining = until.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
}

async fn wait_shutdown(rx: &mut tokio::sync::watch::Receiver<bool>) {
    while !*rx.borrow_and_update() {
        if rx.changed().await.is_err() {
            break;
        }
    }
}

pub fn event_is_relevant(api_dir: &Path, event: &Event) -> bool {
    kind_is_content_change(event.kind) && event.paths.iter().any(|p| is_relevant_path(api_dir, p))
}

pub fn kind_is_content_change(kind: EventKind) -> bool {
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Data(_)) => true,
        EventKind::Modify(ModifyKind::Name(_)) => true,
        EventKind::Modify(ModifyKind::Any) => true,
        EventKind::Modify(ModifyKind::Metadata(_) | ModifyKind::Other) => false,
        EventKind::Access(_) => false,
        EventKind::Other | EventKind::Any => false,
    }
}

pub fn is_relevant_path(api_dir: &Path, path: &Path) -> bool {
    if path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("target" | ".git" | ".erno" | "node_modules")
        )
    }) {
        return false;
    }
    if !path.starts_with(api_dir) {
        return false;
    }
    if is_ephemeral(path) {
        return false;
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs" | "toml") => true,
        _ => path.file_name().is_some_and(|n| n == "build.rs"),
    }
}

fn is_ephemeral(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".swo")
        || name.starts_with('.')
        || name.starts_with('#')
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, MetadataKind};

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
        assert!(!is_relevant_path(
            api,
            Path::new("/proj/api/src/.main.rs.swp")
        ));
        assert!(!is_relevant_path(api, Path::new("/proj/api/Cargo.lock")));
    }

    #[test]
    fn ignores_access_and_metadata_events() {
        assert!(!kind_is_content_change(EventKind::Access(
            AccessKind::Open(AccessMode::Any)
        )));
        assert!(!kind_is_content_change(EventKind::Access(
            AccessKind::Close(AccessMode::Read)
        )));
        assert!(!kind_is_content_change(EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Any)
        )));
        assert!(kind_is_content_change(EventKind::Modify(ModifyKind::Data(
            DataChange::Any
        ))));
        assert!(kind_is_content_change(EventKind::Create(CreateKind::File)));
    }
}
