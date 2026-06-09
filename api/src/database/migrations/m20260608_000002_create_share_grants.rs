use sea_orm_migration::{
    prelude::*,
    schema::{timestamp, timestamp_null, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ShareGrants::Table)
                    .if_not_exists()
                    .col(
                        uuid(ShareGrants::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(uuid(ShareGrants::ShareId).not_null())
                    .col(uuid(ShareGrants::UserId).not_null())
                    .col(timestamp_null(ShareGrants::NotifiedAt))
                    .col(timestamp_null(ShareGrants::RevokedAt))
                    .col(
                        timestamp(ShareGrants::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .col(
                        timestamp(ShareGrants::UpdatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_share_grants_share_id")
                            .from(ShareGrants::Table, ShareGrants::ShareId)
                            .to(Alias::new("shares"), Alias::new("id"))
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_share_grants_user_id")
                            .from(ShareGrants::Table, ShareGrants::UserId)
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
                    .table(ShareGrants::Table)
                    .col(ShareGrants::ShareId)
                    .col(ShareGrants::UserId)
                    .unique()
                    .name("idx_share_grants_share_user")
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(ShareGrants::Table)
                    .col(ShareGrants::UserId)
                    .name("idx_share_grants_user_id")
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TRIGGER update_share_grants_updated_at
                  BEFORE UPDATE ON share_grants
                  FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TRIGGER IF EXISTS update_share_grants_updated_at ON share_grants;",
            )
            .await?;

        manager
            .drop_table(Table::drop().table(ShareGrants::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ShareGrants {
    Table,
    Id,
    ShareId,
    UserId,
    NotifiedAt,
    RevokedAt,
    CreatedAt,
    UpdatedAt,
}
