//! Bounding how much the collector keeps.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! `error_event` is by some distance the highest-volume table in an Erno
//! deployment, so the retention defaults matter more than almost any other
//! setting here.
//!
//! Every delete is batched through an `id IN (SELECT … LIMIT n)` subselect, the
//! same shape the job cleanup uses, so no single statement holds a long lock on
//! a table that ingest is actively writing to.

use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement, Value};

use crate::{
    error_reporting::config::CollectorConfig,
    jobs::advisory_lock::{lock_keys, run_with_advisory_lock},
};

/// Rows removed per statement.
const BATCH_SIZE: u64 = 1_000;
/// Issues examined per pass when trimming over-full ones.
const OFFENDER_LIMIT: u64 = 50;

/// What one sweep removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepOutcome {
    /// Events removed for being older than the retention window.
    pub aged_events: u64,
    /// Events removed for exceeding an issue's cap.
    pub capped_events: u64,
    /// Issues removed for having gone quiet.
    pub stale_issues: u64,
    /// Retired application instances forgotten.
    pub retired_instances: u64,
}

/// Run retention forever, one instance at a time across the deployment.
pub fn spawn(db: DatabaseConnection, config: CollectorConfig, interval: Duration) {
    tokio::spawn(async move {
        run_with_advisory_lock(db, lock_keys::ERROR_RETENTION, "error retention", move |db| {
            let config = config.clone();
            async move {
                loop {
                    tokio::time::sleep(interval).await;
                    match sweep(&db, &config).await {
                        Ok(outcome) => {
                            if outcome != SweepOutcome::default() {
                                // Safe to log: this module's target is ignored
                                // by the capture layer.
                                tracing::info!(
                                    target: "erno::error_reporting::collector",
                                    "🧹 Retention removed {} aged events, {} over-cap events, {} stale issues, {} retired instances",
                                    outcome.aged_events,
                                    outcome.capped_events,
                                    outcome.stale_issues,
                                    outcome.retired_instances
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("error_reporting: retention sweep failed: {e}");
                        }
                    }
                }
            }
        })
        .await;
    });
}

/// One retention pass.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] if any statement fails.
pub async fn sweep(
    db: &DatabaseConnection,
    config: &CollectorConfig,
) -> Result<SweepOutcome, DbErr> {
    // Order matters: trim over-cap issues before removing stale ones, so a
    // single pass does not do work on rows it is about to delete anyway.
    let aged_events = delete_aged_events(db, config.event_retention_days).await?;
    let capped_events = trim_over_cap_issues(db, config.max_events_per_issue).await?;
    let stale_issues = delete_stale_issues(db, config.issue_retention_days).await?;
    let retired_instances =
        super::health::forget_stale(db, config.instance_retention_seconds).await?;

    Ok(SweepOutcome {
        aged_events,
        capped_events,
        stale_issues,
        retired_instances,
    })
}

async fn delete_aged_events(db: &DatabaseConnection, retention_days: u64) -> Result<u64, DbErr> {
    let mut removed = 0;
    loop {
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "DELETE FROM error_event WHERE id IN (
                     SELECT id FROM error_event
                     WHERE created_at < (now() AT TIME ZONE 'utc') - ($1::bigint || ' days')::interval
                     LIMIT $2
                 )",
                vec![
                    Value::BigInt(Some(retention_days as i64)),
                    Value::BigInt(Some(BATCH_SIZE as i64)),
                ],
            ))
            .await?
            .rows_affected();

        removed += affected;
        if affected < BATCH_SIZE {
            return Ok(removed);
        }
    }
}

/// Keep only the newest `cap` events per issue.
///
/// `times_seen` is deliberately left alone: it is a lifetime counter, and a
/// number that shrank as rows were pruned would be baffling in the console.
async fn trim_over_cap_issues(db: &DatabaseConnection, cap: u64) -> Result<u64, DbErr> {
    let offenders = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT issue_id FROM error_event
             GROUP BY issue_id HAVING count(*) > $1::bigint
             LIMIT $2",
            vec![
                Value::BigInt(Some(cap as i64)),
                Value::BigInt(Some(OFFENDER_LIMIT as i64)),
            ],
        ))
        .await?;

    let mut removed = 0;
    for row in offenders {
        let issue_id: uuid::Uuid = row.try_get("", "issue_id")?;
        removed += db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "DELETE FROM error_event WHERE id IN (
                     SELECT id FROM error_event
                     WHERE issue_id = $1
                     ORDER BY created_at DESC
                     OFFSET $2
                 )",
                vec![issue_id.into(), Value::BigInt(Some(cap as i64))],
            ))
            .await?
            .rows_affected();
    }
    Ok(removed)
}

async fn delete_stale_issues(db: &DatabaseConnection, retention_days: u64) -> Result<u64, DbErr> {
    let mut removed = 0;
    loop {
        // Events cascade with the issue.
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "DELETE FROM error_issue WHERE id IN (
                     SELECT id FROM error_issue
                     WHERE last_seen < (now() AT TIME ZONE 'utc') - ($1::bigint || ' days')::interval
                     LIMIT $2
                 )",
                vec![
                    Value::BigInt(Some(retention_days as i64)),
                    Value::BigInt(Some(BATCH_SIZE as i64)),
                ],
            ))
            .await?
            .rows_affected();

        removed += affected;
        if affected < BATCH_SIZE {
            return Ok(removed);
        }
    }
}
