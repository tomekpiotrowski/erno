//! Docs: docs/src/content/docs/monitoring/alerts.md
use sea_orm_migration::{
    prelude::*,
    schema::{big_integer, boolean, double, string, string_null, timestamp, timestamp_null, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AlertRule::Table)
                    .if_not_exists()
                    .col(
                        uuid(AlertRule::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(AlertRule::Name).not_null())
                    .col(boolean(AlertRule::Enabled).not_null().default(true))
                    .col(string(AlertRule::Source).not_null())
                    .col(string(AlertRule::Selector).not_null().default(""))
                    .col(string(AlertRule::Comparator).not_null().default("gt"))
                    .col(double(AlertRule::Threshold).not_null())
                    .col(
                        big_integer(AlertRule::WindowSeconds)
                            .not_null()
                            .default(300),
                    )
                    // How long a breach must persist before it is believed.
                    .col(big_integer(AlertRule::ForSeconds).not_null().default(120))
                    .col(
                        big_integer(AlertRule::RepeatSeconds)
                            .not_null()
                            .default(14_400),
                    )
                    .col(string(AlertRule::Severity).not_null().default("warning"))
                    .col(string_null(AlertRule::NotifyEmail))
                    .col(string_null(AlertRule::NotifyWebhook))
                    .col(timestamp_null(AlertRule::SilenceUntil))
                    .col(string(AlertRule::State).not_null().default("ok"))
                    .col(timestamp_null(AlertRule::StateSince))
                    .col(timestamp_null(AlertRule::LastNotifiedAt))
                    .col(timestamp_null(AlertRule::LastEvaluatedAt))
                    .col(string_null(AlertRule::LastValue))
                    .col(
                        timestamp(AlertRule::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uq-alert_rule-name")
                    .table(AlertRule::Table)
                    .col(AlertRule::Name)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AlertRule::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum AlertRule {
    Table,
    Id,
    Name,
    Enabled,
    Source,
    Selector,
    Comparator,
    Threshold,
    WindowSeconds,
    ForSeconds,
    RepeatSeconds,
    Severity,
    NotifyEmail,
    NotifyWebhook,
    SilenceUntil,
    State,
    StateSince,
    LastNotifiedAt,
    LastEvaluatedAt,
    LastValue,
    CreatedAt,
}
