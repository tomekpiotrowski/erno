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
                    .table(TrialSubscriptions::Table)
                    .if_not_exists()
                    .col(
                        uuid(TrialSubscriptions::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(
                        uuid(TrialSubscriptions::UserId)
                            .not_null()
                            .unique_key(),
                    )
                    .col(string(TrialSubscriptions::Plan).not_null())
                    .col(timestamp(TrialSubscriptions::ActiveUntil).not_null())
                    .col(
                        timestamp(TrialSubscriptions::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_trial_subscriptions_user_id")
                            .from(TrialSubscriptions::Table, TrialSubscriptions::UserId)
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
            .drop_table(Table::drop().table(TrialSubscriptions::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum TrialSubscriptions {
    Table,
    Id,
    UserId,
    Plan,
    ActiveUntil,
    CreatedAt,
}
