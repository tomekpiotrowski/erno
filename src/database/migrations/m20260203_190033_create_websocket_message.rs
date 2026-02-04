use sea_orm_migration::{
    prelude::*,
    schema::{json_binary, timestamp, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create the websocket_message table
        manager
            .create_table(
                Table::create()
                    .table(WebsocketMessage::Table)
                    .if_not_exists()
                    .col(
                        uuid(WebsocketMessage::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(json_binary(WebsocketMessage::RecipientCriteria).not_null())
                    .col(json_binary(WebsocketMessage::Payload).not_null())
                    .col(
                        timestamp(WebsocketMessage::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index for efficient querying
        manager
            .create_index(
                Index::create()
                    .name("idx_websocket_message_created_at")
                    .table(WebsocketMessage::Table)
                    .col(WebsocketMessage::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // Create trigger function that sends NOTIFY on INSERT
        manager
            .get_connection()
            .execute_unprepared(
                r#"
                CREATE OR REPLACE FUNCTION notify_websocket_message()
                RETURNS TRIGGER AS $$
                BEGIN
                    PERFORM pg_notify('websocket_new_message', NEW.id::text);
                    RETURN NEW;
                END;
                $$ LANGUAGE plpgsql;
                "#,
            )
            .await?;

        // Attach trigger to table
        manager
            .get_connection()
            .execute_unprepared(
                r#"
                CREATE TRIGGER websocket_message_notify
                    AFTER INSERT ON websocket_message
                    FOR EACH ROW
                    EXECUTE FUNCTION notify_websocket_message();
                "#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop trigger first
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TRIGGER IF EXISTS websocket_message_notify ON websocket_message",
            )
            .await?;

        // Drop trigger function
        manager
            .get_connection()
            .execute_unprepared("DROP FUNCTION IF EXISTS notify_websocket_message()")
            .await?;

        // Drop index
        manager
            .drop_index(
                Index::drop()
                    .name("idx_websocket_message_created_at")
                    .table(WebsocketMessage::Table)
                    .to_owned(),
            )
            .await?;

        // Drop table
        manager
            .drop_table(Table::drop().table(WebsocketMessage::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum WebsocketMessage {
    Table,
    Id,
    RecipientCriteria,
    Payload,
    CreatedAt,
}
