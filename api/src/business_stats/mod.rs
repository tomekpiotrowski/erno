//! Docs: docs/src/content/docs/api/business-stats.md
pub mod metrics;
pub mod models;
pub mod snapshot_job;

use crate::jobs::{scheduled_job::ScheduledJob, Job};

pub use snapshot_job::{BusinessStatsSnapshotArgs, BusinessStatsSnapshotJob};

/// The recommended daily schedule for [`BusinessStatsSnapshotJob`] — 03:00 UTC,
/// an off-peak hour for most deployments. Push this into your app's
/// `job_schedule()`, alongside registering the job itself in `job_registry()`
/// and adding `"business_stats_snapshot"` to a worker pool's `jobs` list in
/// config (see `docs/src/content/docs/api/business-stats.md`).
#[must_use]
pub fn business_stats_scheduled_job() -> ScheduledJob {
    ScheduledJob {
        name: "business_stats_snapshot".to_string(),
        job_name: BusinessStatsSnapshotJob::<()>::name(),
        arguments: serde_json::Value::Null,
        cron_expression: "0 0 3 * * *".to_string(), // daily at 03:00 UTC
    }
}
