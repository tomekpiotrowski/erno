//! The error-reporting contract shared by an Erno application and the collector.
//!
//! Both halves of error reporting have to agree on this vocabulary, and they
//! are deployed separately — an application and the collector that watches it
//! ship on their own cadences, and after the collector moved to its own tree
//! neither can reach into the other for a type. So the shapes that cross the
//! wire live here, in a crate with no database, no I/O and no configuration.
//!
//! Deliberately small. Request and response bodies are *not* here: the reporter
//! serializes borrowed data and the collector deserializes owned data, so they
//! keep their own paired structs rather than sharing one awkward type.

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
/// the wire — see [`is_in_app`].
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

/// Tracing target the reporter never captures from.
///
/// Reporting's own failures must not become reports, or an outage feeds itself
/// forever. Matched as a **prefix**, so anything beneath it is covered too.
pub const SELF_TARGET: &str = "erno::error_reporting";

/// Tracing target the collector logs its own failures under.
///
/// Deliberately beneath [`SELF_TARGET`] even though the collector now ships as
/// its own crate: when it self-reports, a failed write must not be captured as
/// a new report. The two constants live together so that relationship cannot be
/// broken by renaming a module on one side of the wire.
pub const COLLECTOR_TARGET: &str = "erno::error_reporting::collector";

/// Header carrying the ingest token.
///
/// Here rather than with the collector's authentication because the reporter
/// sets it and the collector reads it; a constant that only one side knows is a
/// typo waiting to become a silent 401.
pub const INGEST_KEY_HEADER: &str = "x-erno-ingest-key";

/// Header a browser uses to label itself `app` or `admin`.
///
/// Honoured only within the set the presented credential allows — a public
/// browser token can never claim [`Source::Api`].
pub const SOURCE_HEADER: &str = "x-erno-source";

/// Whether a file path looks like first-party code rather than a dependency or
/// runtime. Used both for grouping and to dim vendor frames in the UI.
///
/// Shared because both sides run it and must agree: the reporter marks frames
/// before sending, and the collector recomputes `in_app` from `file` rather
/// than trusting the wire. Two copies that drifted would move issues into new
/// groups on a deploy.
#[must_use]
pub fn is_in_app(file: Option<&str>) -> bool {
    let Some(file) = file else {
        return false;
    };
    const VENDOR_MARKERS: [&str; 9] = [
        "node_modules/",
        "/.cargo/registry",
        "/rustc/",
        "zone.js",
        "/rxjs/",
        "core.mjs",
        "/@angular/",
        "polyfills",
        "/std/src/",
    ];
    !VENDOR_MARKERS.iter().any(|m| file.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_paths_are_not_in_app() {
        assert!(is_in_app(Some("/proj/api/src/sync/delta.rs")));
        assert!(!is_in_app(Some("/home/u/.cargo/registry/src/x.rs")));
        assert!(!is_in_app(Some("/app/node_modules/rxjs/index.js")));
        assert!(!is_in_app(None), "a frame with no file is not first-party");
    }

    /// The reporter suppresses by prefix. If the collector's target ever moved
    /// out from under it, a collector that self-reports would capture its own
    /// write failures as new reports and feed the outage forever — silently,
    /// and only in production.
    #[test]
    fn the_collector_target_stays_beneath_the_suppressed_prefix() {
        assert!(COLLECTOR_TARGET.starts_with(SELF_TARGET));
    }

    #[test]
    fn source_and_level_round_trip_through_their_wire_form() {
        for source in [Source::Api, Source::App, Source::Admin] {
            assert_eq!(Source::from_str_opt(source.as_str()), Some(source));
        }
        assert_eq!(Source::from_str_opt("nope"), None);

        for level in [Level::Warning, Level::Error, Level::Fatal] {
            assert_eq!(Level::from_str_or_error(level.as_str()), level);
        }
        // An unknown level reads as `error` rather than being dropped.
        assert_eq!(Level::from_str_or_error("nope"), Level::Error);
        assert!(Level::Warning.rank() < Level::Error.rank());
        assert!(Level::Error.rank() < Level::Fatal.rank());
    }
}
