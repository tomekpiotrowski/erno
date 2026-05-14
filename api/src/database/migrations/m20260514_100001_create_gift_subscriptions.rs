use sea_orm_migration::{
    prelude::*,
    schema::{string, timestamp, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(GiftSubscriptions::Table)
                    .if_not_exists()
                    .col(
                        uuid(GiftSubscriptions::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(uuid(GiftSubscriptions::UserId).not_null())
                    .col(string(GiftSubscriptions::Plan).not_null())
                    .col(timestamp(GiftSubscriptions::ActiveUntil).not_null())
                    .col(
                        timestamp(GiftSubscriptions::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_gift_subscriptions_user_id")
                            .from(GiftSubscriptions::Table, GiftSubscriptions::UserId)
                            .to(Alias::new("users"), Alias::new("id"))
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(GiftSubscriptions::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum GiftSubscriptions {
    Table,
    Id,
    UserId,
    Plan,
    ActiveUntil,
    CreatedAt,
}
