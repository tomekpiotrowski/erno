use sea_orm_migration::{
    prelude::*,
    schema::{string, string_null, timestamp, timestamp_null, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Shares::Table)
                    .if_not_exists()
                    .col(
                        uuid(Shares::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string_null(Shares::TokenHash).unique_key())
                    .col(string(Shares::EntityType).not_null())
                    .col(uuid(Shares::EntityId).not_null())
                    .col(uuid(Shares::OwnerId).not_null())
                    .col(string(Shares::Permission).not_null().default("read"))
                    .col(timestamp_null(Shares::ExpiresAt))
                    .col(timestamp_null(Shares::RevokedAt))
                    .col(
                        timestamp(Shares::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .col(
                        timestamp(Shares::UpdatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_shares_owner_id")
                            .from(Shares::Table, Shares::OwnerId)
                            .to(Alias::new("users"), Alias::new("id"))
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(Shares::Table)
                    .col(Shares::EntityType)
                    .col(Shares::EntityId)
                    .name("idx_shares_entity")
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(Shares::Table)
                    .col(Shares::OwnerId)
                    .name("idx_shares_owner_id")
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TRIGGER update_shares_updated_at
                  BEFORE UPDATE ON shares
                  FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TRIGGER IF EXISTS update_shares_updated_at ON shares;")
            .await?;

        manager
            .drop_table(Table::drop().table(Shares::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum Shares {
    Table,
    Id,
    TokenHash,
    EntityType,
    EntityId,
    OwnerId,
    Permission,
    ExpiresAt,
    RevokedAt,
    CreatedAt,
    UpdatedAt,
}
