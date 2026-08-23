//! First-party error reporting.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! The module is split along a deployment seam:
//!
//! * [`collector`] — ingest, grouping, storage and operator queries. Mounted
//!   only by the `monitoring` binary, which runs on separate infrastructure.
//! * [`reporter`] — the handle every Erno app holds to *send* reports to a
//!   collector, plus the capture hooks (tracing layer, panic hook).
//!
//! [`fingerprint`] and [`scrub`] are shared by both and are deliberately pure:
//! no database, no I/O, no configuration.

pub mod anonymize_user_job;
pub mod collector;
pub mod config;
pub mod fingerprint;
pub mod reporter;
pub mod scrub;

pub use config::{AlertsConfig, CollectorConfig, ErrorReportingConfig, StatusConfig};

use serde::{Deserialize, Serialize};

/// Which component a report came from.
///
/// Never taken from the wire — the collector derives it from the credential the
/// caller presented, so a public browser token cannot claim to be the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The Rust API server.
    Api,
    /// A consumer Angular application.
    App,
    /// The operator admin panel.
    Admin,
}

impl Source {
    /// Stable wire/database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::App => "app",
            Self::Admin => "admin",
        }
    }

    /// Parse from the database/wire representation.
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "api" => Some(Self::Api),
            "app" => Some(Self::App),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Severity of a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Something unexpected that did not break the operation.
    Warning,
    /// A handled failure.
    #[default]
    Error,
    /// A panic or otherwise unrecoverable failure.
    Fatal,
}

impl Level {
    /// Stable wire/database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Fatal => "fatal",
        }
    }

    /// Parse from the database/wire representation, defaulting to [`Level::Error`].
    #[must_use]
    pub fn from_str_or_error(value: &str) -> Self {
        match value {
            "warning" => Self::Warning,
            "fatal" => Self::Fatal,
            _ => Self::Error,
        }
    }

    /// Ordering for `min_level` comparisons: warning < error < fatal.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Warning => 0,
            Self::Error => 1,
            Self::Fatal => 2,
        }
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One frame of a stack trace.
///
/// `in_app` is recomputed by the collector from `file` rather than trusted from
/// the wire — see [`fingerprint::is_in_app`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// Function or method name, if the runtime provided one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// Source file or bundle URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 1-indexed line number. Stored for display; never part of the fingerprint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 1-indexed column number. Stored for display; never part of the fingerprint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Whether this frame is first-party code rather than a dependency.
    #[serde(default)]
    pub in_app: bool,
}

/// A report in its internal, wire-agnostic form.
///
/// Both halves build these: the reporter from a captured panic, tracing event,
/// or browser exception; the collector from a deserialized ingest envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedError {
    /// Which component reported. Assigned by the collector from the caller's
    /// credential, never taken from the wire.
    pub source: Source,
    /// Severity.
    pub level: Level,
    /// Exception type, error class, or tracing target.
    pub error_type: String,
    /// The error message, already scrubbed.
    pub message: String,
    /// Raw stack text, if the runtime provided one.
    pub stack: Option<String>,
    /// Parsed frames, nearest-to-throw first.
    pub frames: Vec<Frame>,
    /// Structured context: url, route, method, user agent, arbitrary extras.
    /// Always a JSON object, already scrubbed.
    pub context: serde_json::Value,
    /// Release the reporting build identifies as.
    pub release: Option<String>,
    /// Deployment environment.
    pub environment: Option<String>,
    /// Affected user, when the reporter knew one.
    pub user_id: Option<uuid::Uuid>,
    /// Affected user's email, denormalized by the reporting app because the
    /// collector has no users table to look it up in.
    pub user_email: Option<String>,
    /// Reporter's IP, only populated when the collector is configured to keep it.
    pub client_ip: Option<String>,
    /// Explicit grouping override supplied by the reporter.
    pub client_fingerprint: Option<Vec<String>>,
    /// When the error happened, clamped to a sane window by the collector.
    pub timestamp: chrono::NaiveDateTime,
}

impl CapturedError {
    /// A minimal report, for the capture paths that have little to work with.
    #[must_use]
    pub fn new(source: Source, level: Level, error_type: String, message: String) -> Self {
        Self {
            source,
            level,
            error_type,
            message,
            stack: None,
            frames: Vec::new(),
            context: serde_json::Value::Object(serde_json::Map::new()),
            release: None,
            environment: None,
            user_id: None,
            user_email: None,
            client_ip: None,
            client_fingerprint: None,
            timestamp: chrono::Utc::now().naive_utc(),
        }
    }

    /// The `file` of the call site, used to group stackless captures.
    #[must_use]
    pub fn call_site(&self) -> Option<&str> {
        self.context.get("file").and_then(serde_json::Value::as_str)
    }
}
