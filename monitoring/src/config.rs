//! Monitoring-specific configuration.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Rides on Erno's `ExtraConfig` mechanism: `Config<MonitorConfig>` flattens
//! this into the same file as `[database]`, `[jobs]`, `[admin]`, and the rest,
//! so a monitoring deployment is configured exactly like any other Erno app.

use crate::collector::config::CollectorConfig;
use serde::{Deserialize, Serialize};

/// The `[collector]` section of a monitoring deployment's config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitorConfig {
    /// Error-reporting collector settings.
    #[serde(default)]
    pub collector: CollectorConfig,
}
