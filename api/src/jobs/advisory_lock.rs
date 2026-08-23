use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, Statement};
use std::{future::Future, time::Duration};
use tokio::time::sleep;
use tracing::{debug, error, warn};

/// Advisory lock keys for different background tasks
pub mod lock_keys {
    /// Lock key for the job scheduler
    pub const SCHEDULER: i64 = 0x5343_4845_4455_4C45; // "SCHEDULE" in hex

    /// Lock key for job cleanup task
    pub const CLEANUP: i64 = 0x434C_4541_4E55_5000; // "CLEANUP" in hex

    /// Lock key for stuck job recovery
    pub const RECOVERY: i64 = 0x5245_434F_5645_5259; // "RECOVERY" in hex

    /// Lock key for error-report retention, held by the monitoring collector.
    pub const ERROR_RETENTION: i64 = 0x4552_524F_5252_4554; // "ERRORRET" in hex

    /// Lock key for the uptime prober, held by the monitoring collector.
    ///
    /// Singleton on purpose: every replica probing would multiply load on the
    /// target and make the recorded ratios meaningless.
    pub const UPTIME: i64 = 0x5550_5449_4D45_4348; // "UPTIMECH" in hex

    /// Lock key for the status-page publisher.
    ///
    /// Singleton so replicas cannot race to write the same document.
    pub const STATUS_PUBLISH: i64 = 0x5354_4154_5553_5047; // "STATUSPG" in hex

    /// Lock key for the alert evaluator.
    ///
    /// Singleton because replicas each notifying would multiply every alert by
    /// the replica count.
    pub const ALERTING: i64 = 0x414C_4552_5452_554C; // "ALERTRUL" in hex
}
/// Tries to acquire a `PostgreSQL` advisory lock
pub async fn try_acquire_lock(db: &DatabaseConnection, key: i64) -> Result<bool, DbErr> {
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "SELECT pg_try_advisory_lock($1)",
        [key.into()],
    );

    let result = db.query_one(stmt).await?;
    Ok(result
        .and_then(|row| row.try_get_by_index::<bool>(0).ok())
        .unwrap_or(false))
}

/// Explicitly releases a `PostgreSQL` advisory lock
pub async fn release_lock(db: &DatabaseConnection, key: i64) -> Result<bool, DbErr> {
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "SELECT pg_advisory_unlock($1)",
        [key.into()],
    );

    let result = db.query_one(stmt).await?;
    Ok(result
        .and_then(|row| row.try_get_by_index::<bool>(0).ok())
        .unwrap_or(false))
}

/// Runs a task with advisory lock protection
/// Only one instance across all application instances will run the task at a time
pub async fn run_with_advisory_lock<F, Fut>(
    db: DatabaseConnection,
    lock_key: i64,
    task_name: &str,
    task_fn: F,
) where
    F: Fn(DatabaseConnection) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut restart_count = 0;

    loop {
        match try_acquire_lock(&db, lock_key).await {
            Ok(true) => {
                debug!("🔒 Acquired advisory lock for {}", task_name);

                // Run the task
                task_fn(db.clone()).await;

                // Task completed (likely due to error), release lock and restart
                match release_lock(&db, lock_key).await {
                    Ok(true) => {
                        debug!("🔓 Released advisory lock for {}", task_name);
                    }
                    Ok(false) => {
                        debug!(
                            "🔓 Advisory lock for {} was already released (possibly by connection close)",
                            task_name
                        );
                    }
                    Err(e) => {
                        warn!("Failed to release advisory lock for {}: {}", task_name, e);
                    }
                }

                restart_count += 1;
                error!(
                    "💥 {} crashed (restart #{}) - restarting in 10s...",
                    task_name, restart_count
                );

                sleep(Duration::from_secs(10)).await;
            }
            Ok(false) => {
                debug!(
                    "🔒 Advisory lock for {} held by another instance, waiting...",
                    task_name
                );

                // Add jitter to prevent thundering herd
                let sleep_duration =
                    Duration::from_secs(5) + Duration::from_millis(fastrand::u64(0..2000));
                sleep(sleep_duration).await;
            }
            Err(e) => {
                error!(
                    "❌ Failed to acquire advisory lock for {}: {}",
                    task_name, e
                );
                sleep(Duration::from_secs(10)).await;
            }
        }
    }
}
