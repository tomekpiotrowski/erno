//! Publishing the status snapshot.
//!
//! Docs: docs/src/content/docs/monitoring/status-page.md
//!
//! Writes a static JSON document on an interval. The status page reads that
//! document and nothing else, which is what lets it keep telling the truth
//! while the collector is down — the last published document is still there.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sea_orm::{DatabaseConnection, EntityTrait};

use super::service::build_snapshot;
use crate::error_reporting::collector::models::project;
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
/// `output_path` is a directory. Until snapshots are project-scoped, this
/// writes a single `{dir}/status.json`. Per-slug files would currently mix
/// every project's components under one name.
///
/// # Errors
///
/// Returns a message describing what failed — a database error, a serialisation
/// error, or a filesystem error.
pub async fn publish_once(db: &DatabaseConnection, config: &StatusConfig) -> Result<(), String> {
    let output = Path::new(config.output_path.trim());
    let enabled = project::Entity::find()
        .all(db)
        .await
        .map_err(|e| format!("listing projects: {e}"))?
        .into_iter()
        .any(|p| p.status_enabled);
    if !enabled {
        // `[collector.status] enabled` only turns the publisher on; a project
        // still has to opt in. Say so once, or an operator who set the config
        // key is left wondering why no document ever appears.
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            eprintln!(
                "status: publishing is on, but no project has status_enabled set — \
                 nothing to publish"
            );
        });
        return Ok(());
    }

    let snapshot = build_snapshot(db, &config.name, config.refresh_seconds)
        .await
        .map_err(|e| format!("building snapshot: {e}"))?;
    write_snapshot(&output.join("status.json"), &snapshot).await
}

async fn write_snapshot(
    path: &Path,
    snapshot: &super::snapshot::StatusSnapshot,
) -> Result<(), String> {
    let json =
        serde_json::to_vec_pretty(snapshot).map_err(|e| format!("serialising snapshot: {e}"))?;
    write_atomically(path, &json)
        .await
        .map_err(|e| format!("writing {}: {e}", path.display()))
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
