//! Publishing the status snapshot.
//!
//! Docs: docs/src/content/docs/monitoring/status-page.md
//!
//! Writes a static JSON document on an interval. The status page reads that
//! document and nothing else, which is what lets it keep telling the truth
//! while the collector is down — the last published document is still there.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sea_orm::DatabaseConnection;

use super::service::build_snapshot;
use crate::error_reporting::config::StatusConfig;
use crate::jobs::advisory_lock::{lock_keys, run_with_advisory_lock};

/// Start publishing.
pub fn spawn(db: DatabaseConnection, config: StatusConfig) {
    if !config.is_active() {
        return;
    }

    tokio::spawn(async move {
        // One publisher across the deployment: replicas racing to write the
        // same file would occasionally leave a torn document.
        run_with_advisory_lock(
            db,
            lock_keys::STATUS_PUBLISH,
            "status publisher",
            move |db| {
                let config = config.clone();
                async move {
                    let interval = Duration::from_secs(config.refresh_seconds.max(5));
                    loop {
                        if let Err(e) = publish_once(&db, &config).await {
                            eprintln!("status: could not publish snapshot: {e}");
                            metrics::counter!("erno_status_publish_total", "result" => "failed")
                                .increment(1);
                        } else {
                            metrics::counter!("erno_status_publish_total", "result" => "ok")
                                .increment(1);
                        }
                        tokio::time::sleep(interval).await;
                    }
                }
            },
        )
        .await;
    });
}

/// Build and write one snapshot.
///
/// # Errors
///
/// Returns a message describing what failed — a database error, a serialisation
/// error, or a filesystem error.
pub async fn publish_once(db: &DatabaseConnection, config: &StatusConfig) -> Result<(), String> {
    let snapshot = build_snapshot(db, &config.name, config.refresh_seconds)
        .await
        .map_err(|e| format!("building snapshot: {e}"))?;

    let json =
        serde_json::to_vec_pretty(&snapshot).map_err(|e| format!("serialising snapshot: {e}"))?;

    write_atomically(Path::new(&config.output_path), &json)
        .await
        .map_err(|e| format!("writing {}: {e}", config.output_path))
}

/// Write via a temporary file and rename.
///
/// A reader that catches a half-written document would show nonsense at exactly
/// the wrong moment; rename is atomic within a filesystem, so a reader sees
/// either the old document or the new one.
async fn write_atomically(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }

    let temporary: PathBuf = path.with_extension("json.tmp");
    tokio::fs::write(&temporary, contents).await?;
    tokio::fs::rename(&temporary, path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writing_creates_missing_directories_and_leaves_no_temporary() {
        let dir = std::env::temp_dir().join(format!("erno-status-{}", uuid::Uuid::new_v4()));
        let path = dir.join("nested").join("status.json");

        write_atomically(&path, b"{\"state\":\"operational\"}")
            .await
            .expect("write");

        assert_eq!(
            tokio::fs::read_to_string(&path).await.expect("read"),
            "{\"state\":\"operational\"}"
        );
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the temporary file must be renamed away, not left behind"
        );

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn republishing_replaces_the_previous_document() {
        let dir = std::env::temp_dir().join(format!("erno-status-{}", uuid::Uuid::new_v4()));
        let path = dir.join("status.json");

        write_atomically(&path, b"old").await.expect("write");
        write_atomically(&path, b"new").await.expect("rewrite");

        assert_eq!(tokio::fs::read_to_string(&path).await.expect("read"), "new");
        tokio::fs::remove_dir_all(&dir).await.ok();
    }
}
