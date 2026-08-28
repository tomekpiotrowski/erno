//! Erno monitoring collector.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! A thin Erno application: it mounts the collector half of
//! `erno::error_reporting` and nothing else. Everything underneath — config
//! loading, migrations, the job queue, the mailer, metrics, health checks,
//! operator Basic auth — comes from the library, which is what makes running
//! monitoring as a separate deployment cheap rather than a second framework.

use axum::Router;
use erno::{
    app::App,
    app_info::AppInfo,
    boot::{boot, BootConfig},
    error_reporting::collector::collector_router,
    jobs::{job_registry::JobRegistry, scheduled_job::ScheduledJob},
};

mod config;
mod migrator;

#[cfg(test)]
mod tests;

pub use config::MonitorConfig;
pub use migrator::MonitorMigrator;

/// Routes served by the monitoring deployment.
///
/// The framework nests this under `/api`, so the collector's ingest endpoint
/// lands at `POST /api/errors`.
fn app_router(app: App<MonitorConfig>) -> Router {
    let collector = app.config.extra.collector.clone();
    let mut router = Router::new();

    if let Some(routes) = collector_router(app, collector) {
        router = router.merge(routes);
    }

    router
}

fn job_registry() -> JobRegistry<MonitorConfig> {
    JobRegistry::new()
}

fn job_schedule() -> Vec<ScheduledJob> {
    vec![]
}

fn boot_config() -> BootConfig<MonitorConfig> {
    let app_info = AppInfo::new(
        "erno-monitoring",
        env!("CARGO_PKG_VERSION"),
        "Erno monitoring collector",
    );

    BootConfig::new(app_info, app_router, job_registry(), job_schedule()).skip_default_cors()
}

#[tokio::main]
async fn main() {
    boot::<MonitorMigrator, MonitorConfig>(boot_config()).await;
}
