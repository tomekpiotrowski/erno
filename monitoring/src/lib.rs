//! Erno monitoring collector.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! An Erno application that owns the receiving half of error reporting:
//! ingest, grouping, storage, alerting and the operator API. Everything
//! underneath — config loading, migrations, the job queue, the mailer, metrics,
//! health checks, operator Basic auth — comes from the `erno` library, which is
//! what makes running monitoring as a separate deployment cheap rather than a
//! second framework.
//!
//! It deliberately does not live inside that library. The collector watches
//! applications and must not go down with them, so it ships on its own cadence
//! and shares only [`erno_error_reporting_types`], the contract that crosses
//! the wire.

use axum::Router;
use erno::{
    app::App,
    app_info::AppInfo,
    boot::BootConfig,
    jobs::{job_registry::JobRegistry, scheduled_job::ScheduledJob},
};

pub mod collector;
pub mod config;
pub mod fingerprint;
pub mod scrub;

mod migrator;

#[cfg(test)]
mod tests;

use collector::collector_router;

pub use config::MonitorConfig;
pub use migrator::MonitorMigrator;

/// Routes served by the monitoring deployment.
///
/// The framework nests this under `/api`, so the collector's ingest endpoint
/// lands at `POST /api/errors`.
pub fn app_router(app: App<MonitorConfig>) -> Router {
    let collector = app.config.extra.collector.clone();
    let mut router = Router::new();

    if let Some(routes) = collector_router(app, collector) {
        router = router.merge(routes);
    }

    router
}

pub fn job_registry() -> JobRegistry<MonitorConfig> {
    JobRegistry::new()
}

pub fn job_schedule() -> Vec<ScheduledJob> {
    vec![]
}

/// Everything the framework needs to start this deployment.
#[must_use]
pub fn boot_config() -> BootConfig<MonitorConfig> {
    let app_info = AppInfo::new(
        "erno-monitoring",
        env!("CARGO_PKG_VERSION"),
        "Erno monitoring collector",
    );

    BootConfig::new(app_info, app_router, job_registry(), job_schedule()).skip_default_cors()
}

/// Run the collector.
///
/// Migrations, config loading and the server all come from the framework; this
/// only names the migrator and the config shape.
pub async fn boot() {
    erno::boot::boot::<MonitorMigrator, MonitorConfig>(boot_config()).await;
}
