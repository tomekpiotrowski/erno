use sea_orm_migration::{
    prelude::*,
    schema::{json_binary, string, timestamp, uuid, uuid_null},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AdminEvent::Table)
                    .if_not_exists()
                    .col(
                        uuid(AdminEvent::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(AdminEvent::Name).not_null())
                    .col(uuid_null(AdminEvent::UserId))
                    .col(json_binary(AdminEvent::Payload).not_null())
                    .col(
                        timestamp(AdminEvent::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-admin_event-created_at")
                    .table(AdminEvent::Table)
                    .col(AdminEvent::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-admin_event-name")
                    .table(AdminEvent::Table)
                    .col(AdminEvent::Name)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AdminEvent::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum AdminEvent {
    Table,
    Id,
    Name,
    UserId,
    Payload,
    CreatedAt,
}
