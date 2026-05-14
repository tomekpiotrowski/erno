use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};
use sqlx::Row;
use std::time::Duration;
use tokio::time::sleep;
use tracing::warn;

use crate::{
    config::MetricsConfig,
    database::models::{
        job::{self, Entity as JobEntity},
        job_status::JobStatus,
    },
    metrics::collector::CollectorRegistry,
};

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
        collectors.collect_all(&db).await;
    }
}

fn collect_db_pool_stats(db: &DatabaseConnection) {
    let pool = db.get_postgres_connection_pool();
    metrics::gauge!("db_pool_connections_total").set(f64::from(pool.size()));
    metrics::gauge!("db_pool_connections_idle").set(pool.num_idle() as f64);
}

async fn collect_table_counts(db: &DatabaseConnection, config: &MetricsConfig) {
    let pool = db.get_postgres_connection_pool();
    for table in &config.table_counts {
        let query = format!("SELECT COUNT(*) FROM \"{table}\"");
        match sqlx::query(&query).fetch_one(pool).await {
            Ok(row) => {
                let count: i64 = row.try_get(0).unwrap_or(0);
                metrics::gauge!("db_table_count", "table" => table.clone())
                    .set(count as f64);
            }
            Err(e) => {
                warn!("Failed to count table '{table}': {e}");
            }
        }
    }
}

async fn collect_job_queue_stats(db: &DatabaseConnection) {
    let pending_statuses = [JobStatus::Pending, JobStatus::PendingRetry, JobStatus::Running];

    for status in &pending_statuses {
        let status_label = format!("{status}");

        // Get all job types with this status and count them
        let jobs = JobEntity::find()
            .select_only()
            .column(job::Column::Type)
            .filter(job::Column::Status.eq(status.clone()))
            .into_tuple::<String>()
            .all(db)
            .await
            .unwrap_or_default();

        // Aggregate counts per job type
        let mut counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for job_type in jobs {
            *counts.entry(job_type).or_insert(0) += 1;
        }

        for (job_type, count) in counts {
            metrics::gauge!("jobs_pending_count",
                "job_type" => job_type,
                "status" => status_label.clone(),
            )
            .set(count as f64);
        }
    }
}
