//! Erno subsystem health.
//!
//! Docs: docs/src/content/docs/monitoring/subsystem-health.md
//!
//! The signals here exist because Erno knows things about itself that a generic
//! monitoring tool cannot: what a job's lifecycle looks like, that a `running`
//! row older than its timeout means a worker died, that a growing
//! `sync_push_queue` means clients are about to see stale data.
//!
//! Readings are published two ways, and a deployment may use either or both:
//! as Prometheus gauges for scraping, and as a snapshot pushed to a monitoring
//! collector. Push matters because it works when the application is not
//! reachable from outside — and because a heartbeat that *stops* is itself the
//! clearest signal that something is very wrong.

pub mod gather;
pub mod reporter;
pub mod snapshot;

pub use gather::{export_gauges, gather};
pub use reporter::{spawn_health_reporter, HealthReporterConfig};
pub use snapshot::{
    HealthSnapshot, HealthState, HealthThresholds, JobHealth, SubsystemStatus, SyncHealth,
};
