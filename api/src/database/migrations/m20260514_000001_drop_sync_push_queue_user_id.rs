use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(
            "DROP INDEX IF EXISTS idx_sync_push_queue_user_seq; \
             ALTER TABLE sync_push_queue DROP COLUMN IF EXISTS user_id;",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(
            "ALTER TABLE sync_push_queue ADD COLUMN IF NOT EXISTS user_id UUID; \
             CREATE INDEX IF NOT EXISTS idx_sync_push_queue_user_seq ON sync_push_queue (user_id, sync_seq);",
        )
        .await?;
        Ok(())
    }
}
