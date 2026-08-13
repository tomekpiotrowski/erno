use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sqlx::Row;
use std::time::Duration;
use tokio::time::sleep;
use tracing::warn;

use crate::{config::MetricsConfig, metrics::collector::CollectorRegistry};

pub async fn db_stats_task(
    db: DatabaseConnection,
    config: MetricsConfig,
    collectors: CollectorRegistry,
) {
    let interval = Duration::from_secs(config.db_stats_interval_seconds);
    loop {
        sleep(interval).await;
        collect_db_pool_stats(&db);
        collect_table_counts(&db, &config).await;
        collect_job_queue_stats(&db).await;
        collect_user_gauges(&db).await;
        collectors.collect_all(&db).await;
    }
}

fn collect_db_pool_stats(db: &DatabaseConnection) {
    let pool = db.get_postgres_connection_pool();
    metrics::gauge!("db_pool_connections_total").set(f64::from(pool.size()));
    metrics::gauge!("db_pool_connections_idle").set(pool.num_idle() as f64);
}

/// Approximate row counts from `pg_stat_user_tables.n_live_tup` — never `COUNT(*)`.
pub async fn collect_table_counts(db: &DatabaseConnection, config: &MetricsConfig) {
    let pool = db.get_postgres_connection_pool();
    if config.table_counts.is_empty() {
        match sqlx::query("SELECT relname, n_live_tup FROM pg_stat_user_tables")
            .fetch_all(pool)
            .await
        {
            Ok(rows) => {
                for row in rows {
                    let table: String = row.try_get(0).unwrap_or_default();
                    let count: i64 = row.try_get(1).unwrap_or(0);
                    metrics::gauge!("db_table_count", "table" => table).set(count as f64);
                }
            }
            Err(e) => warn!("Failed to read pg_stat_user_tables: {e}"),
        }
        return;
    }

    match sqlx::query("SELECT relname, n_live_tup FROM pg_stat_user_tables WHERE relname = ANY($1)")
        .bind(&config.table_counts)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => {
            for row in rows {
                let table: String = row.try_get(0).unwrap_or_default();
                let count: i64 = row.try_get(1).unwrap_or(0);
                metrics::gauge!("db_table_count", "table" => table).set(count as f64);
            }
        }
        Err(e) => warn!("Failed to read pg_stat_user_tables: {e}"),
    }
}

pub async fn collect_job_queue_stats(db: &DatabaseConnection) {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT type, status, COUNT(*)::bigint AS n \
             FROM job \
             WHERE status IN ('pending', 'pending_retry', 'running') \
             GROUP BY type, status",
        ))
        .await;
    match rows {
        Ok(rows) => {
            for r in rows {
                let job_type: String = r.try_get("", "type").unwrap_or_default();
                let status: String = r.try_get("", "status").unwrap_or_default();
                let n: i64 = r.try_get("", "n").unwrap_or(0);
                metrics::gauge!("jobs_pending_count",
                    "job_type" => job_type,
                    "status" => status,
                )
                .set(n as f64);
            }
        }
        Err(e) => warn!("Failed to collect job queue stats: {e}"),
    }
}

/// Exact gauges on the small `users` table (and modest subscription tables).
pub async fn collect_user_gauges(db: &DatabaseConnection) {
    let Ok(total) = scalar(db, "SELECT COUNT(*)::bigint AS n FROM users").await else {
        return;
    };
    metrics::gauge!("erno_users_total").set(total as f64);

    if let Ok(n) = scalar(
        db,
        "SELECT COUNT(*)::bigint AS n FROM users WHERE email_verified_at IS NOT NULL",
    )
    .await
    {
        metrics::gauge!("erno_users_email_verified").set(n as f64);
    }

    if let Ok(rows) = db
        .query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT subscription_type, subscription_plan, COUNT(*)::bigint AS n \
             FROM users GROUP BY 1, 2",
        ))
        .await
    {
        let mut paid = 0.0;
        let mut trial = 0.0;
        let mut gift = 0.0;
        let mut none = 0.0;
        for r in rows {
            let n: i64 = r.try_get("", "n").unwrap_or(0);
            let kind: Option<String> = r.try_get("", "subscription_type").ok();
            let plan: Option<String> = r.try_get("", "subscription_plan").ok();
            match kind.as_deref() {
                Some("stripe") => {
                    paid += n as f64;
                    if let Some(plan) = plan {
                        metrics::gauge!("erno_users_paid", "plan" => plan).set(n as f64);
                    }
                }
                Some("trial") => trial += n as f64,
                Some("gift") => gift += n as f64,
                _ => none += n as f64,
            }
        }
        metrics::gauge!("erno_users_paid").set(paid);
        metrics::gauge!("erno_users_trial").set(trial);
        metrics::gauge!("erno_users_gift").set(gift);
        metrics::gauge!("erno_users_none").set(none);
    }

    for (metric, interval) in [
        ("erno_users_active_1d", "1 day"),
        ("erno_users_active_7d", "7 days"),
        ("erno_users_active_30d", "30 days"),
    ] {
        let sql = format!(
            "SELECT COUNT(*)::bigint AS n FROM users WHERE last_active_at > NOW() - INTERVAL '{interval}'"
        );
        if let Ok(n) = scalar(db, &sql).await {
            metrics::gauge!(metric).set(n as f64);
        }
    }

    if let Ok(n) = scalar(
        db,
        "SELECT COUNT(DISTINCT user_id)::bigint AS n FROM stripe_subscriptions WHERE status = 'past_due'",
    )
    .await
    {
        metrics::gauge!("erno_users_past_due").set(n as f64);
    }
    if let Ok(n) = scalar(
        db,
        "SELECT COUNT(DISTINCT user_id)::bigint AS n FROM stripe_subscriptions WHERE status = 'canceled'",
    )
    .await
    {
        metrics::gauge!("erno_users_canceled").set(n as f64);
    }
    if let Ok(n) = scalar(
        db,
        "SELECT COUNT(*)::bigint AS n FROM stripe_subscriptions \
         WHERE status = 'active' AND cancel_at_period_end = true",
    )
    .await
    {
        metrics::gauge!("erno_users_cancel_at_period_end").set(n as f64);
    }

    if let Ok(n) = scalar(
        db,
        "SELECT COALESCE(SUM(byte_size), 0)::bigint AS n FROM files",
    )
    .await
    {
        metrics::gauge!("erno_storage_bytes").set(n as f64);
    }
    if let Ok(n) = scalar(
        db,
        "SELECT COUNT(DISTINCT file_id)::bigint AS n FROM file_attachments",
    )
    .await
    {
        metrics::gauge!("erno_files_total").set(n as f64);
    }
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> Result<i64, sea_orm::DbErr> {
    let row = db
        .query_one(Statement::from_string(DbBackend::Postgres, sql.to_string()))
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<i64>("", "n").ok())
        .unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use axum::Router;

    use super::*;
    use crate::{app::App, database::migrations::Migrator, tests::setup_test::setup_test};

    fn test_router(_app: App) -> Router {
        Router::new()
    }
    fn no_fixtures(
        db: &sea_orm::DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let _ = db;
        })
    }

    #[tokio::test]
    async fn table_counts_use_pg_stat_not_full_count() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let config = MetricsConfig {
            table_counts: vec!["users".to_string()],
            ..MetricsConfig::default()
        };
        collect_table_counts(&t.db, &config).await;
        collect_job_queue_stats(&t.db).await;
        collect_user_gauges(&t.db).await;
    }
}
