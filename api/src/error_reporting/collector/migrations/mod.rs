//! Collector schema.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! These migrations are **deliberately absent** from
//! [`crate::database::migrations::erno_migrations`]. They belong to the
//! monitoring deployment's own database; adding them to the framework list
//! would give every application deployment two large tables it never writes to.

pub use sea_orm_migration::prelude::*;

pub mod m20260823_090000_create_error_issue;
pub mod m20260823_090100_create_error_event;
pub mod m20260823_120000_create_release;
pub mod m20260823_130000_create_app_health;
pub mod m20260823_140000_create_uptime;
pub mod m20260823_150000_create_status_page;
pub mod m20260823_160000_create_alert_rule;
pub mod m20260823_170000_widen_alert_selector;

/// Every collector migration, in order. A monitoring binary chains these into
/// its own `MigratorTrait` implementation.
#[must_use]
pub fn collector_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
        Box::new(m20260823_090000_create_error_issue::Migration),
        Box::new(m20260823_090100_create_error_event::Migration),
        Box::new(m20260823_120000_create_release::Migration),
        Box::new(m20260823_130000_create_app_health::Migration),
        Box::new(m20260823_140000_create_uptime::Migration),
        Box::new(m20260823_150000_create_status_page::Migration),
        Box::new(m20260823_160000_create_alert_rule::Migration),
        Box::new(m20260823_170000_widen_alert_selector::Migration),
    ]
}

/// A ready-made migrator for a monitoring deployment that stores nothing else.
pub struct CollectorMigrator;

#[async_trait::async_trait]
impl MigratorTrait for CollectorMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        collector_migrations()
    }
}
