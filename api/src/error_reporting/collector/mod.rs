//! The receiving half of error reporting.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Mounted only by the `monitoring` binary, which runs on infrastructure
//! separate from the application it watches. Nothing here is compiled into an
//! application deployment's request path.

pub mod alerting;
pub mod alerts;
pub mod auth;
pub mod cors;
pub mod dto;
pub mod handlers;
pub mod health;
pub mod ingest;
pub mod migrations;
pub mod models;
pub mod operator;
pub mod operator_dto;
pub mod projects;
pub mod releases;
pub mod retention;
pub mod router;
pub mod service;
pub mod state;
pub mod status;
pub mod uptime;

pub use migrations::{collector_migrations, CollectorMigrator};
pub use router::collector_router;
pub use state::CollectorState;
