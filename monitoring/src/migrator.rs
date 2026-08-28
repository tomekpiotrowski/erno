//! Schema for a monitoring deployment.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! The monitoring service is an ordinary Erno application, so it needs the
//! framework's own tables — the job queue it schedules cleanup on, the email
//! outbox its alerts go through — chained ahead of the collector's.

use crate::collector::collector_migrations;
use erno::database::migrations::erno_migrations;
use sea_orm_migration::{MigrationTrait, MigratorTrait};

/// Framework migrations followed by the collector's own.
pub struct MonitorMigrator;

#[async_trait::async_trait]
impl MigratorTrait for MonitorMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = erno_migrations();
        migrations.extend(collector_migrations());
        migrations
    }
}
