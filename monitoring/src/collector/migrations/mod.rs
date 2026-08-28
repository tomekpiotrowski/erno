//! Collector schema.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! These migrations are **deliberately absent** from
//! [`erno::database::migrations::erno_migrations`]. They belong to the
//! monitoring deployment's own database; adding them to the framework list
//! would give every application deployment two large tables it never writes to.

pub use sea_orm_migration::prelude::*;

pub mod m20260828_000000_init;

/// Every collector migration, in order. A monitoring binary chains these into
/// its own `MigratorTrait` implementation.
#[must_use]
pub fn collector_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(m20260828_000000_init::Migration)]
}

/// A ready-made migrator for a monitoring deployment that stores nothing else.
pub struct CollectorMigrator;

#[async_trait::async_trait]
impl MigratorTrait for CollectorMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        collector_migrations()
    }
}
