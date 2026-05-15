use erno::database::migrations::erno_migrations;
use sea_orm_migration::{MigrationTrait, MigratorTrait};

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        erno_migrations()
        // Add app-specific migrations by chaining:
        // .into_iter()
        //     .chain([Box::new(m20260101_000001_create_posts::Migration) as Box<dyn MigrationTrait>])
        //     .collect()
    }
}
