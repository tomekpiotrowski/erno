use sea_orm_migration::{
    prelude::*,
    schema::{json_binary, string, timestamp, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        // Global monotonic sequence for all syncable entities
        conn.execute_unprepared("CREATE SEQUENCE IF NOT EXISTS erno_sync_clock START 1 INCREMENT 1")
            .await?;

        // Event log table — never deleted, used for both WS push and delta sync
        manager
            .create_table(
                Table::create()
                    .table(SyncPushQueue::Table)
                    .if_not_exists()
                    .col(
                        uuid(SyncPushQueue::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(SyncPushQueue::EntityType).not_null())
                    .col(uuid(SyncPushQueue::EntityId).not_null())
                    .col(ColumnDef::new(SyncPushQueue::SyncSeq).big_integer().not_null())
                    .col(string(SyncPushQueue::Operation).not_null())
                    .col(json_binary(SyncPushQueue::Snapshot).not_null())
                    // NULL user_id = broadcast to all connected users
                    .col(ColumnDef::new(SyncPushQueue::UserId).uuid())
                    .col(
                        timestamp(SyncPushQueue::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(SyncPushQueue::Table)
                    .name("idx_sync_push_queue_sync_seq")
                    .col(SyncPushQueue::SyncSeq)
                    .to_owned(),
            )
            .await?;

        // Composite index for efficient per-user delta sync queries
        manager
            .create_index(
                Index::create()
                    .table(SyncPushQueue::Table)
                    .name("idx_sync_push_queue_user_seq")
                    .col(SyncPushQueue::UserId)
                    .col(SyncPushQueue::SyncSeq)
                    .to_owned(),
            )
            .await?;

        // Trigger to NOTIFY listeners on every insert
        conn.execute_unprepared(
            r#"
            CREATE OR REPLACE FUNCTION erno_notify_sync_new_event()
            RETURNS trigger AS $$
            BEGIN
                PERFORM pg_notify('sync_new_event', '');
                RETURN NEW;
            END;
            $$ LANGUAGE plpgsql;

            CREATE TRIGGER sync_push_queue_notify
            AFTER INSERT ON sync_push_queue
            FOR EACH ROW EXECUTE FUNCTION erno_notify_sync_new_event();
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(
            "DROP TRIGGER IF EXISTS sync_push_queue_notify ON sync_push_queue; \
             DROP FUNCTION IF EXISTS erno_notify_sync_new_event;",
        )
        .await?;
        manager
            .drop_table(Table::drop().table(SyncPushQueue::Table).to_owned())
            .await?;
        conn.execute_unprepared("DROP SEQUENCE IF EXISTS erno_sync_clock")
            .await?;
        Ok(())
    }
}

#[derive(Iden)]
enum SyncPushQueue {
    Table,
    Id,
    EntityType,
    EntityId,
    SyncSeq,
    Operation,
    Snapshot,
    UserId,
    CreatedAt,
}
