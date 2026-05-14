pub use sea_orm_migration::prelude::*;

mod m20250805_180000_create_update_at_trigger;
mod m20250805_192936_create_job;
mod m20260203_190033_create_websocket_message;
mod m20260513_000001_create_users;
mod m20260513_000002_create_user_tokens;
mod m20260513_100000_create_sync_infrastructure;
mod m20260514_000001_drop_sync_push_queue_user_id;
mod m20260514_000002_add_token_version_to_users;
mod m20260514_100000_create_stripe_subscriptions;
mod m20260514_100001_create_gift_subscriptions;
mod m20260514_100002_create_trial_subscriptions;
mod m20260514_100003_add_subscription_cache_to_users;
mod m20260514_200000_create_files;
mod m20260514_200001_create_file_attachments;
mod m20260515_000001_add_refresh_token_type;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250805_180000_create_update_at_trigger::Migration),
            Box::new(m20250805_192936_create_job::Migration),
            Box::new(m20260203_190033_create_websocket_message::Migration),
            Box::new(m20260513_000001_create_users::Migration),
            Box::new(m20260513_000002_create_user_tokens::Migration),
            Box::new(m20260513_100000_create_sync_infrastructure::Migration),
            Box::new(m20260514_000001_drop_sync_push_queue_user_id::Migration),
            Box::new(m20260514_000002_add_token_version_to_users::Migration),
            Box::new(m20260514_100000_create_stripe_subscriptions::Migration),
            Box::new(m20260514_100001_create_gift_subscriptions::Migration),
            Box::new(m20260514_100002_create_trial_subscriptions::Migration),
            Box::new(m20260514_100003_add_subscription_cache_to_users::Migration),
            Box::new(m20260514_200000_create_files::Migration),
            Box::new(m20260514_200001_create_file_attachments::Migration),
            Box::new(m20260515_000001_add_refresh_token_type::Migration),
        ]
    }
}

pub struct Migrator;
