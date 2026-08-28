//! Configuration for both halves of error reporting.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! [`ErrorReportingConfig`] is held by every Erno app and describes how to
//! *send* reports. [`CollectorConfig`] is held by the `monitoring` binary and
//! describes how to *receive* and store them. They live together so the
//! defaults on either side stay in step.
//!
//! Every field carries a `#[serde(default)]`, so an existing `config/*.toml`
//! that has never heard of error reporting keeps booting unchanged.

use serde::{Deserialize, Serialize};

use super::Level;
use crate::health::snapshot::HealthThresholds;

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

/// The bundled Prometheus, for `promql` alert rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrometheusConfig {
    /// Base URL, e.g. `http://<release>-prometheus:9090`.
    #[serde(default)]
    pub url: String,
}

impl PrometheusConfig {
    /// The URL to query, or `None` when unconfigured.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        let trimmed = self.url.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }
}

/// Collector-side configuration, held by the `monitoring` binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorConfig {
    /// Mounts the ingest route and starts the writer. When false the route is
    /// not registered at all, matching the `admin_router` idiom.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Optional plaintext used only when seeding the empty `project` table.
    /// Ignored once any project exists.
    #[serde(default)]
    pub seed: CollectorSeedConfig,

    /// Bounded ingest queue depth.
    #[serde(default = "default_queue_capacity")]
    pub queue_capacity: usize,

    /// Maximum reports written per flush.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// How long the writer waits to accumulate a batch.
    #[serde(default = "default_flush_interval_ms")]
    pub flush_interval_ms: u64,

    /// Write on the request's own connection instead of via the background
    /// writer. Required in tests, where each case runs inside a single
    /// connection's transaction that is rolled back afterwards.
    #[serde(default)]
    pub sync_writes: bool,

    /// Events accepted per request; the rest are counted and dropped.
    #[serde(default = "default_max_events_per_request")]
    pub max_events_per_request: usize,

    /// Event *rows* stored per fingerprint per flush. Counts still accumulate
    /// in full, so a render loop costs a bounded number of rows.
    #[serde(default = "default_max_events_per_flush_per_issue")]
    pub max_events_per_flush_per_issue: usize,

    /// Request body cap in bytes.
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,

    /// Persist the reporter's IP. Off by default: an IP is a stronger
    /// identifier than anything else here and is rarely needed for triage.
    #[serde(default)]
    pub store_client_ip: bool,

    /// Days of individual events kept.
    #[serde(default = "default_event_retention_days")]
    pub event_retention_days: u64,

    /// Days an untouched issue is kept after it was last seen.
    #[serde(default = "default_issue_retention_days")]
    pub issue_retention_days: u64,

    /// Hard cap on stored events per issue.
    #[serde(default = "default_max_events_per_issue")]
    pub max_events_per_issue: u64,

    /// New-issue email alerting.
    #[serde(default)]
    pub alerts: AlertsConfig,

    /// When an application instance counts as degraded or down.
    #[serde(default)]
    pub health: HealthThresholds,

    /// Public status page publishing.
    #[serde(default)]
    pub status: StatusConfig,

    /// The bundled Prometheus, for alert rules whose source is `promql`.
    ///
    /// Reached in-cluster, never through the ingress. Empty disables the
    /// `promql` alert source; rules using it then read as not breaching.
    #[serde(default)]
    pub prometheus: PrometheusConfig,

    /// Forget an instance that has not reported for this long, so retired
    /// replicas do not pile up in the console after every rolling deploy.
    #[serde(default = "default_instance_retention_seconds")]
    pub instance_retention_seconds: i64,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            seed: CollectorSeedConfig::default(),
            queue_capacity: default_queue_capacity(),
            batch_size: default_batch_size(),
            flush_interval_ms: default_flush_interval_ms(),
            sync_writes: false,
            max_events_per_request: default_max_events_per_request(),
            max_events_per_flush_per_issue: default_max_events_per_flush_per_issue(),
            max_body_bytes: default_max_body_bytes(),
            store_client_ip: false,
            event_retention_days: default_event_retention_days(),
            issue_retention_days: default_issue_retention_days(),
            max_events_per_issue: default_max_events_per_issue(),
            alerts: AlertsConfig::default(),
            health: HealthThresholds::default(),
            status: StatusConfig::default(),
            prometheus: PrometheusConfig::default(),
            instance_retention_seconds: default_instance_retention_seconds(),
        }
    }
}

/// Public status page publishing.
///
/// The page reads a published document rather than calling this service, so it
/// keeps working when the collector does not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusConfig {
    /// Master switch.
    #[serde(default)]
    pub enabled: bool,

    /// Product or organisation name shown in the page header.
    #[serde(default = "default_status_name")]
    pub name: String,

    /// Where the snapshot is written. In production this should be a location
    /// served from somewhere other than this deployment — object storage behind
    /// a CDN — or the page goes down with the collector.
    #[serde(default = "default_status_output_path")]
    pub output_path: String,

    /// Seconds between publications. Also told to the page, so it can judge
    /// staleness without hard-coding this value.
    #[serde(default = "default_status_refresh_seconds")]
    pub refresh_seconds: u64,
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            name: default_status_name(),
            output_path: default_status_output_path(),
            refresh_seconds: default_status_refresh_seconds(),
        }
    }
}

impl StatusConfig {
    /// Whether the publisher should run.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && !self.output_path.trim().is_empty()
    }
}

fn default_status_name() -> String {
    "Service status".to_string()
}

fn default_status_output_path() -> String {
    "status/".to_string()
}

/// Plaintext ingest tokens used only to seed the first `monitoring` project.
///
/// Ignored when the `project` table is already non-empty. Not a fallback for
/// ingest — lookup is always by stored hash.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectorSeedConfig {
    /// Overrides the server token hashed into the seed row.
    #[serde(default)]
    pub server_token: String,
    /// Overrides the browser token hashed into the seed row.
    #[serde(default)]
    pub browser_token: String,
}

const fn default_status_refresh_seconds() -> u64 {
    30
}

/// New-issue email alerting, with the throttle that keeps a bad deploy from
/// mailing hundreds of times.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertsConfig {
    /// Master switch.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Operator address. Empty leaves alerting inert, which is the default so
    /// a fresh install never mails anyone unexpectedly.
    #[serde(default)]
    pub recipient: String,

    /// Minimum severity that triggers an alert.
    #[serde(default = "default_alert_min_level")]
    pub min_level: String,

    /// Individual alerts allowed per window before suppression kicks in.
    #[serde(default = "default_alert_max_per_window")]
    pub max_per_window: usize,

    /// Length of the throttle window.
    #[serde(default = "default_alert_window_minutes")]
    pub window_minutes: u64,

    /// Floor on the gap between two alerts, so even an allowed burst is paced.
    #[serde(default = "default_alert_min_interval_seconds")]
    pub min_interval_seconds: u64,
}

impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            recipient: String::new(),
            min_level: default_alert_min_level(),
            max_per_window: default_alert_max_per_window(),
            window_minutes: default_alert_window_minutes(),
            min_interval_seconds: default_alert_min_interval_seconds(),
        }
    }
}

impl AlertsConfig {
    /// Whether alerts should actually be sent.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && !self.recipient.trim().is_empty()
    }

    /// Parsed minimum level, defaulting to [`Level::Error`].
    #[must_use]
    pub fn minimum_level(&self) -> Level {
        Level::from_str_or_error(&self.min_level)
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
const fn default_max_events_per_request() -> usize {
    20
}
const fn default_max_events_per_flush_per_issue() -> usize {
    10
}
const fn default_max_body_bytes() -> usize {
    64 * 1024
}
const fn default_event_retention_days() -> u64 {
    30
}
const fn default_issue_retention_days() -> u64 {
    90
}
const fn default_max_events_per_issue() -> u64 {
    500
}
fn default_alert_min_level() -> String {
    "error".to_string()
}
const fn default_instance_retention_seconds() -> i64 {
    24 * 60 * 60
}
const fn default_alert_max_per_window() -> usize {
    10
}
const fn default_alert_window_minutes() -> u64 {
    60
}
const fn default_alert_min_interval_seconds() -> u64 {
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

    #[test]
    fn collector_defaults_are_conservative() {
        let config: CollectorConfig = toml::from_str("").expect("valid");
        assert!(!config.store_client_ip);
        assert!(!config.sync_writes);
        assert_eq!(config.max_events_per_flush_per_issue, 10);
        assert_eq!(config.status.output_path, "status/");
        // Alerts are configured on but inert without a recipient.
        assert!(config.alerts.enabled);
        assert!(!config.alerts.is_active());
    }

    #[test]
    fn leftover_collector_tokens_are_ignored() {
        let config: CollectorConfig = toml::from_str(
            r#"
            server_token = "old-server"
            browser_token = "old-browser"
            "#,
        )
        .expect("unknown keys are ignored");
        assert!(config.seed.server_token.is_empty());
        assert!(config.seed.browser_token.is_empty());
    }

    #[test]
    fn alert_minimum_level_parses() {
        let config: AlertsConfig = toml::from_str(r#"min_level = "fatal""#).expect("valid");
        assert_eq!(config.minimum_level(), Level::Fatal);
        let config: AlertsConfig = toml::from_str(r#"min_level = "nonsense""#).expect("valid");
        assert_eq!(config.minimum_level(), Level::Error);
    }

    #[test]
    fn nested_alerts_table_parses() {
        let config: CollectorConfig = toml::from_str(
            r#"
            [seed]
            server_token = "s"
            [alerts]
            recipient = "ops@example.com"
            max_per_window = 3
        "#,
        )
        .expect("valid");
        assert_eq!(config.seed.server_token, "s");
        assert!(config.alerts.is_active());
        assert_eq!(config.alerts.max_per_window, 3);
        assert_eq!(config.alerts.window_minutes, 60);
    }
}
