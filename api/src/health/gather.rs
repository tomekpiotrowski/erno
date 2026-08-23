//! Reading subsystem health out of the database.
//!
//! Docs: docs/src/content/docs/monitoring/subsystem-health.md

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement, Value};

use super::snapshot::{DatabaseHealth, HealthSnapshot, JobHealth, SyncHealth, WebSocketHealth};

/// Gather a reading for this process.
///
/// Every query is an aggregate over an indexed column, so this is cheap enough
/// to run on a short interval.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] if a query fails.
pub async fn gather(
    db: &DatabaseConnection,
    instance: String,
    release: Option<String>,
    environment: String,
    job_timeout_seconds: u32,
    websocket_connections: i64,
) -> Result<HealthSnapshot, DbErr> {
    Ok(HealthSnapshot {
        instance,
        release,
        environment,
        reported_at: Utc::now().naive_utc(),
        jobs: gather_jobs(db, job_timeout_seconds).await?,
        sync: gather_sync(db).await?,
        database: gather_database(db),
        websocket: WebSocketHealth {
            connections: websocket_connections,
        },
    })
}

async fn gather_jobs(
    db: &DatabaseConnection,
    job_timeout_seconds: u32,
) -> Result<JobHealth, DbErr> {
    // One round trip: counting each status separately would be five.
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT
                 count(*) FILTER (WHERE status = 'pending')::bigint        AS pending,
                 count(*) FILTER (WHERE status = 'pending_retry')::bigint  AS pending_retry,
                 count(*) FILTER (WHERE status = 'running')::bigint        AS running,
                 count(*) FILTER (
                     WHERE status = 'failed'
                       AND updated_at >= (now() AT TIME ZONE 'utc') - interval '1 hour'
                 )::bigint                                                 AS failed_last_hour,
                 count(*) FILTER (
                     WHERE status = 'running'
                       AND updated_at < (now() AT TIME ZONE 'utc') - ($1::bigint || ' seconds')::interval
                 )::bigint                                                 AS stuck_running,
                 EXTRACT(EPOCH FROM (
                     (now() AT TIME ZONE 'utc') - min(created_at) FILTER (
                         WHERE status IN ('pending', 'pending_retry')
                     )
                 ))::bigint                                                AS oldest_pending_age
             FROM job",
            vec![Value::BigInt(Some(i64::from(job_timeout_seconds)))],
        ))
        .await?;

    let Some(row) = row else {
        return Ok(JobHealth::default());
    };

    Ok(JobHealth {
        pending: row.try_get("", "pending")?,
        pending_retry: row.try_get("", "pending_retry")?,
        running: row.try_get("", "running")?,
        failed_last_hour: row.try_get("", "failed_last_hour")?,
        stuck_running: row.try_get("", "stuck_running")?,
        // NULL when nothing is waiting, which is the healthy case.
        oldest_pending_age_seconds: row.try_get("", "oldest_pending_age").ok(),
    })
}

async fn gather_sync(db: &DatabaseConnection) -> Result<SyncHealth, DbErr> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT
                 count(*)::bigint AS depth,
                 EXTRACT(EPOCH FROM ((now() AT TIME ZONE 'utc') - min(created_at)))::bigint AS oldest_age
             FROM sync_push_queue",
        ))
        .await?;

    let Some(row) = row else {
        return Ok(SyncHealth::default());
    };

    Ok(SyncHealth {
        push_queue_depth: row.try_get("", "depth")?,
        oldest_push_age_seconds: row.try_get("", "oldest_age").ok(),
    })
}

fn gather_database(db: &DatabaseConnection) -> DatabaseHealth {
    let pool = db.get_postgres_connection_pool();
    DatabaseHealth {
        pool_size: pool.size(),
        pool_idle: u32::try_from(pool.num_idle()).unwrap_or(u32::MAX),
    }
}

/// Publish a snapshot as Prometheus gauges.
///
/// Push to the collector and scrape by Prometheus are not alternatives: a
/// deployment may use either or both, and both read the same numbers.
pub fn export_gauges(snapshot: &HealthSnapshot) {
    let jobs = &snapshot.jobs;
    metrics::gauge!("erno_jobs_waiting").set((jobs.pending + jobs.pending_retry) as f64);
    metrics::gauge!("erno_jobs_running").set(jobs.running as f64);
    metrics::gauge!("erno_jobs_failed_last_hour").set(jobs.failed_last_hour as f64);
    metrics::gauge!("erno_jobs_stuck_running").set(jobs.stuck_running as f64);
    metrics::gauge!("erno_jobs_oldest_waiting_seconds")
        .set(jobs.oldest_pending_age_seconds.unwrap_or(0) as f64);

    metrics::gauge!("erno_sync_push_queue_depth").set(snapshot.sync.push_queue_depth as f64);
    metrics::gauge!("erno_sync_oldest_push_seconds")
        .set(snapshot.sync.oldest_push_age_seconds.unwrap_or(0) as f64);

    metrics::gauge!("erno_websocket_connections").set(snapshot.websocket.connections as f64);
}
