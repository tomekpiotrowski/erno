//! Docs: docs/src/content/docs/monitoring/uptime.md
use sea_orm_migration::{
    prelude::*,
    schema::{
        boolean, integer, integer_null, string, string_null, text_null, timestamp, timestamp_null,
        uuid,
    },
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(UptimeCheck::Table)
                    .if_not_exists()
                    .col(
                        uuid(UptimeCheck::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(UptimeCheck::Name).not_null())
                    .col(string(UptimeCheck::Url).not_null())
                    .col(string(UptimeCheck::Method).not_null().default("GET"))
                    .col(integer(UptimeCheck::ExpectedStatus).not_null().default(200))
                    .col(integer(UptimeCheck::TimeoutMs).not_null().default(10_000))
                    .col(integer(UptimeCheck::IntervalSeconds).not_null().default(60))
                    .col(boolean(UptimeCheck::Enabled).not_null().default(true))
                    .col(text_null(UptimeCheck::AssertBodyContains))
                    // Flap damping: one dropped packet must not page anyone.
                    .col(integer(UptimeCheck::FailureThreshold).not_null().default(2))
                    .col(
                        string(UptimeCheck::CurrentState)
                            .not_null()
                            .default("unknown"),
                    )
                    .col(
                        integer(UptimeCheck::ConsecutiveFailures)
                            .not_null()
                            .default(0),
                    )
                    .col(timestamp_null(UptimeCheck::StateChangedAt))
                    .col(timestamp_null(UptimeCheck::LastCheckedAt))
                    .col(
                        timestamp(UptimeCheck::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uq-uptime_check-name")
                    .table(UptimeCheck::Table)
                    .col(UptimeCheck::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UptimeResult::Table)
                    .if_not_exists()
                    .col(
                        uuid(UptimeResult::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(uuid(UptimeResult::CheckId).not_null())
                    .col(boolean(UptimeResult::Ok).not_null())
                    .col(integer_null(UptimeResult::StatusCode))
                    .col(integer(UptimeResult::DurationMs).not_null())
                    .col(string_null(UptimeResult::Error))
                    .col(timestamp(UptimeResult::CheckedAt).not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_uptime_result_check_id")
                            .from(UptimeResult::Table, UptimeResult::CheckId)
                            .to(UptimeCheck::Table, UptimeCheck::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-uptime_result-check_id_checked_at")
                    .table(UptimeResult::Table)
                    .col(UptimeResult::CheckId)
                    .col(UptimeResult::CheckedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-uptime_result-checked_at")
                    .table(UptimeResult::Table)
                    .col(UptimeResult::CheckedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(UptimeResult::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(UptimeCheck::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum UptimeCheck {
    Table,
    Id,
    Name,
    Url,
    Method,
    ExpectedStatus,
    TimeoutMs,
    IntervalSeconds,
    Enabled,
    AssertBodyContains,
    FailureThreshold,
    CurrentState,
    ConsecutiveFailures,
    StateChangedAt,
    LastCheckedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum UptimeResult {
    Table,
    Id,
    CheckId,
    Ok,
    StatusCode,
    DurationMs,
    Error,
    CheckedAt,
}
