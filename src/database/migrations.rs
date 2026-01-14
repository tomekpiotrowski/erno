pub use sea_orm_migration::prelude::*;

mod m20250805_180000_create_update_at_trigger;
mod m20250805_192936_create_job;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250805_180000_create_update_at_trigger::Migration),
            Box::new(m20250805_192936_create_job::Migration),
        ]
    }
}

pub struct Migrator;
