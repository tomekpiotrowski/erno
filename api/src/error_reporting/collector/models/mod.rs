//! Collector entities.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Module-local rather than in `database/models/` because they live in the
//! monitoring deployment's database, which no application deployment has.

pub mod alert_rule;
pub mod app_health;
pub mod error_event;
pub mod error_issue;
pub mod release;
pub mod status_component;
pub mod status_incident;
pub mod status_incident_update;
pub mod uptime_check;
pub mod uptime_result;

pub use error_issue::IssueStatus;
