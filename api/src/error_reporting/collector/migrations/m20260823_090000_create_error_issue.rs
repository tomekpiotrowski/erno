//! Docs: docs/src/content/docs/monitoring/error-reporting.md
use sea_orm_migration::{
    prelude::*,
    schema::{big_integer, string, string_null, timestamp, timestamp_null, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ErrorIssue::Table)
                    .if_not_exists()
                    .col(
                        uuid(ErrorIssue::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(ErrorIssue::Fingerprint).not_null())
                    .col(string(ErrorIssue::Source).not_null())
                    .col(string(ErrorIssue::ErrorType).not_null())
                    .col(string(ErrorIssue::Title).not_null())
                    .col(string_null(ErrorIssue::Culprit))
                    .col(string(ErrorIssue::Level).not_null().default("error"))
                    .col(string(ErrorIssue::Status).not_null().default("unresolved"))
                    .col(big_integer(ErrorIssue::TimesSeen).not_null().default(0))
                    .col(timestamp(ErrorIssue::FirstSeen).not_null())
                    .col(timestamp(ErrorIssue::LastSeen).not_null())
                    .col(string_null(ErrorIssue::FirstRelease))
                    .col(string_null(ErrorIssue::LastRelease))
                    .col(string_null(ErrorIssue::Environment))
                    .col(timestamp_null(ErrorIssue::ResolvedAt))
                    .col(timestamp_null(ErrorIssue::AlertSentAt))
                    .col(
                        timestamp(ErrorIssue::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        // The upsert's conflict target. Required, not merely an optimisation:
        // `ON CONFLICT (fingerprint)` will not compile without it.
        manager
            .create_index(
                Index::create()
                    .name("uq-error_issue-fingerprint")
                    .table(ErrorIssue::Table)
                    .col(ErrorIssue::Fingerprint)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Serves the default operator list query.
        manager
            .create_index(
                Index::create()
                    .name("idx-error_issue-status_last_seen")
                    .table(ErrorIssue::Table)
                    .col(ErrorIssue::Status)
                    .col(ErrorIssue::LastSeen)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-error_issue-source_last_seen")
                    .table(ErrorIssue::Table)
                    .col(ErrorIssue::Source)
                    .col(ErrorIssue::LastSeen)
                    .to_owned(),
            )
            .await?;

        // Retention sweep.
        manager
            .create_index(
                Index::create()
                    .name("idx-error_issue-last_seen")
                    .table(ErrorIssue::Table)
                    .col(ErrorIssue::LastSeen)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ErrorIssue::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum ErrorIssue {
    Table,
    Id,
    Fingerprint,
    Source,
    ErrorType,
    Title,
    Culprit,
    Level,
    Status,
    TimesSeen,
    FirstSeen,
    LastSeen,
    FirstRelease,
    LastRelease,
    Environment,
    ResolvedAt,
    AlertSentAt,
    CreatedAt,
}
