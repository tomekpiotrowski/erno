//! Configuration for reporting this application's own errors.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! [`ErrorReportingConfig`] is held by every Erno app and describes how to
//! *send* reports. The receiving half lives with the collector, in the
//! separately deployed `erno-monitoring` crate.
//!
//! Every field carries a `#[serde(default)]`, so an existing `config/*.toml`
//! that has never heard of error reporting keeps booting unchanged.

use serde::{Deserialize, Serialize};

/// App-side configuration: how this process reports its own errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorReportingConfig {
    /// Master switch. When false nothing is captured and no sender is spawned.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Absolute base URL of the collector, e.g. `https://monitoring.example.com`.
    /// Empty disables reporting even when `enabled` is true, which is the
    /// default so a fresh app never tries to phone home.
    #[serde(default)]
    pub collector_url: String,

    /// Shared secret for the trusted server-to-server ingest path. Unlike the
    /// browser token this is a real secret and belongs in SOPS.
    #[serde(default)]
    pub ingest_token: String,

    /// Bounded in-memory queue depth. Full means drop, never block.
    #[serde(default = "default_queue_capacity")]
    pub queue_capacity: usize,

    /// Maximum reports per outbound request.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// How long the sender waits to accumulate a batch.
    #[serde(default = "default_flush_interval_ms")]
    pub flush_interval_ms: u64,

    /// Hard timeout on each outbound request, so a black-holed collector
    /// cannot pin the sender task.
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,

    /// Consecutive failures before the circuit breaker opens.
    #[serde(default = "default_circuit_breaker_failures")]
    pub circuit_breaker_failures: u32,

    /// How long the breaker stays open before probing again.
    #[serde(default = "default_circuit_breaker_cooldown_ms")]
    pub circuit_breaker_cooldown_ms: u64,

    /// Capture `tracing::error!` events.
    ///
    /// Defaults to true, but ships as `false` in the production template: it
    /// turns every pre-existing error log into an issue on the first deploy,
    /// which is the fastest way to teach a team to ignore the tool.
    #[serde(default = "default_true")]
    pub capture_tracing_errors: bool,

    /// Install a panic hook and the `CatchPanicLayer`.
    #[serde(default = "default_true")]
    pub capture_panics: bool,

    /// Reserved for the v2 5xx-response middleware. Not yet implemented.
    #[serde(default)]
    pub capture_5xx: bool,

    /// Tracing targets never captured, e.g. a chatty third-party crate.
    #[serde(default)]
    pub ignore_targets: Vec<String>,

    /// Push subsystem health readings to the collector.
    ///
    /// Independent of error capture: a deployment may want liveness without
    /// error reporting, or the reverse.
    #[serde(default = "default_true")]
    pub report_health: bool,

    /// Seconds between health readings.
    #[serde(default = "default_health_interval_seconds")]
    pub health_interval_seconds: u64,
}

impl Default for ErrorReportingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            collector_url: String::new(),
            ingest_token: String::new(),
            queue_capacity: default_queue_capacity(),
            batch_size: default_batch_size(),
            flush_interval_ms: default_flush_interval_ms(),
            request_timeout_ms: default_request_timeout_ms(),
            circuit_breaker_failures: default_circuit_breaker_failures(),
            circuit_breaker_cooldown_ms: default_circuit_breaker_cooldown_ms(),
            capture_tracing_errors: true,
            capture_panics: true,
            capture_5xx: false,
            ignore_targets: Vec::new(),
            report_health: true,
            health_interval_seconds: default_health_interval_seconds(),
        }
    }
}

impl ErrorReportingConfig {
    /// Whether this process should actually send anything.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && !self.collector_url.trim().is_empty()
    }

    /// Full URL of the health-reporting endpoint.
    #[must_use]
    pub fn health_endpoint(&self) -> String {
        format!(
            "{}/api/collector/health",
            self.collector_url.trim_end_matches('/')
        )
    }

    /// Full URL of the ingest endpoint.
    ///
    /// Must stay in step with where the monitoring deployment mounts the
    /// collector: the framework nests an app router under `/api`, so the
    /// collector's ingest route lands at `/api/errors`.
    #[must_use]
    pub fn ingest_endpoint(&self) -> String {
        format!("{}/api/errors", self.collector_url.trim_end_matches('/'))
    }

    /// Full URL for anonymising one user's stored events.
    ///
    /// The trusted machine route — authenticated by `ingest_token`, not by
    /// operator credentials, because the application calls it during account
    /// deletion.
    #[must_use]
    pub fn user_events_endpoint(&self, user_id: uuid::Uuid) -> String {
        format!(
            "{}/api/collector/users/{user_id}/events",
            self.collector_url.trim_end_matches('/')
        )
    }

    /// Whether a tracing target is excluded from capture.
    #[must_use]
    pub fn is_ignored_target(&self, target: &str) -> bool {
        self.ignore_targets
            .iter()
            .any(|ignored| target.starts_with(ignored.as_str()))
    }
}

const fn default_true() -> bool {
    true
}

const fn default_queue_capacity() -> usize {
    1024
}

const fn default_batch_size() -> usize {
    200
}

const fn default_flush_interval_ms() -> u64 {
    1000
}

const fn default_request_timeout_ms() -> u64 {
    5000
}

const fn default_circuit_breaker_failures() -> u32 {
    5
}

const fn default_circuit_breaker_cooldown_ms() -> u64 {
    60_000
}

const fn default_health_interval_seconds() -> u64 {
    30
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_table_deserialises_to_defaults() {
        let config: ErrorReportingConfig = toml::from_str("").expect("empty table is valid");
        assert!(config.enabled);
        assert_eq!(config.queue_capacity, 1024);
        assert!(config.capture_panics);
        // No collector URL means reporting stays off despite `enabled`.
        assert!(!config.is_active());
    }

    #[test]
    fn a_partial_table_keeps_the_other_defaults() {
        let config: ErrorReportingConfig =
            toml::from_str(r#"collector_url = "https://m.test""#).expect("valid");
        assert!(config.is_active());
        assert_eq!(config.batch_size, 200);
        assert_eq!(config.ingest_endpoint(), "https://m.test/api/errors");
    }

    #[test]
    fn a_trailing_slash_does_not_double_up() {
        let config: ErrorReportingConfig =
            toml::from_str(r#"collector_url = "https://m.test/""#).expect("valid");
        assert_eq!(config.ingest_endpoint(), "https://m.test/api/errors");
    }

    #[test]
    fn ignored_targets_match_by_prefix() {
        let config: ErrorReportingConfig =
            toml::from_str(r#"ignore_targets = ["noisy::crate"]"#).expect("valid");
        assert!(config.is_ignored_target("noisy::crate::inner"));
        assert!(!config.is_ignored_target("erno::sync"));
    }
}
