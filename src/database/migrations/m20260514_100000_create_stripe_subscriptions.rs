use sea_orm::{DbBackend, Schema};
use sea_orm_migration::{
    prelude::*,
    schema::{boolean, string, timestamp, uuid},
};

use crate::billing::models::subscription_status::SubscriptionStatus;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let schema = Schema::new(DbBackend::Postgres);
        manager
            .create_type(schema.create_enum_from_active_enum::<SubscriptionStatus>())
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(StripeSubscriptions::Table)
                    .if_not_exists()
                    .col(
                        uuid(StripeSubscriptions::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(uuid(StripeSubscriptions::UserId).not_null())
                    .col(string(StripeSubscriptions::StripeCustomerId).not_null())
                    .col(
                        string(StripeSubscriptions::StripeSubscriptionId)
                            .not_null()
                            .unique_key(),
                    )
                    .col(string(StripeSubscriptions::Plan).not_null())
                    .col(
                        ColumnDef::new(StripeSubscriptions::Status)
                            .custom(Alias::new("subscription_status"))
                            .not_null(),
                    )
                    .col(timestamp(StripeSubscriptions::CurrentPeriodStart).not_null())
                    .col(timestamp(StripeSubscriptions::CurrentPeriodEnd).not_null())
                    .col(
                        boolean(StripeSubscriptions::CancelAtPeriodEnd)
                            .not_null()
                            .default(false),
                    )
                    .col(
                        timestamp(StripeSubscriptions::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .col(
                        timestamp(StripeSubscriptions::UpdatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_stripe_subscriptions_user_id")
                            .from(StripeSubscriptions::Table, StripeSubscriptions::UserId)
                            .to(Alias::new("users"), Alias::new("id"))
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TRIGGER update_stripe_subscriptions_updated_at
                  BEFORE UPDATE ON stripe_subscriptions
                  FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(StripeSubscriptions::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_type(
                extension::postgres::TypeDropStatement::new()
                    .name(Alias::new("subscription_status"))
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum StripeSubscriptions {
    Table,
    Id,
    UserId,
    StripeCustomerId,
    StripeSubscriptionId,
    Plan,
    Status,
    CurrentPeriodStart,
    CurrentPeriodEnd,
    CancelAtPeriodEnd,
    CreatedAt,
    UpdatedAt,
}
