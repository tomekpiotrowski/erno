//! First-party error reporting.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! This is the *sending* half only. [`reporter`] is the handle every Erno app
//! holds to send reports to a collector, plus the capture hooks (tracing layer,
//! panic hook).
//!
//! Ingest, grouping, storage and the operator API live in the `erno-monitoring`
//! crate, which is deployed on its own infrastructure precisely so it does not
//! go down with the application it is watching. Nothing here reaches into it.
//!
//! The vocabulary that crosses the wire between the two — [`Source`], [`Level`],
//! [`Frame`], [`CapturedError`], the ingest headers and [`is_in_app`] — lives in
//! the `erno-error-reporting-types` crate that both depend on, and is
//! re-exported here so callers keep one path to it.

pub mod anonymize_user_job;
pub mod config;
pub mod reporter;

pub use config::ErrorReportingConfig;

pub use erno_error_reporting_types::{
    is_in_app, CapturedError, Frame, Level, Source, COLLECTOR_TARGET, INGEST_KEY_HEADER,
    SELF_TARGET, SOURCE_HEADER,
};
