pub use sea_orm_migration::prelude::*;

mod m20250805_180000_create_update_at_trigger;
mod m20250805_192936_create_job;
mod m20260203_190033_create_websocket_message;
mod m20260513_000001_create_users;
mod m20260513_000002_create_user_tokens;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250805_180000_create_update_at_trigger::Migration),
            Box::new(m20250805_192936_create_job::Migration),
            Box::new(m20260203_190033_create_websocket_message::Migration),
            Box::new(m20260513_000001_create_users::Migration),
            Box::new(m20260513_000002_create_user_tokens::Migration),
        ]
    }
}

pub struct Migrator;
