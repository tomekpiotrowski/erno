//! Docs: docs/src/content/docs/monitoring/error-reporting.md
use sea_orm_migration::{
    prelude::*,
    schema::{
        json_binary, json_binary_null, string, string_null, text, text_null, timestamp, uuid,
        uuid_null,
    },
};

use super::m20260823_090000_create_error_issue::ErrorIssue;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ErrorEvent::Table)
                    .if_not_exists()
                    .col(
                        uuid(ErrorEvent::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(uuid(ErrorEvent::IssueId).not_null())
                    // Denormalised so event queries never need the join.
                    .col(string(ErrorEvent::Source).not_null())
                    .col(string(ErrorEvent::Level).not_null())
                    .col(string(ErrorEvent::ErrorType).not_null())
                    .col(text(ErrorEvent::Message).not_null())
                    .col(text_null(ErrorEvent::Stack))
                    .col(json_binary_null(ErrorEvent::Frames))
                    .col(json_binary(ErrorEvent::Context).not_null())
                    .col(string_null(ErrorEvent::Release))
                    .col(string_null(ErrorEvent::Environment))
                    // No foreign key: the users table lives in the application's
                    // database, not this one.
                    .col(uuid_null(ErrorEvent::UserId))
                    .col(string_null(ErrorEvent::UserEmail))
                    .col(string_null(ErrorEvent::ClientIp))
                    .col(timestamp(ErrorEvent::CreatedAt).not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_error_event_issue_id")
                            .from(ErrorEvent::Table, ErrorEvent::IssueId)
                            .to(ErrorIssue::Table, ErrorIssue::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Drives the issue detail page.
        manager
            .create_index(
                Index::create()
                    .name("idx-error_event-issue_id_created_at")
                    .table(ErrorEvent::Table)
                    .col(ErrorEvent::IssueId)
                    .col(ErrorEvent::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // Retention sweep and the global time series.
        manager
            .create_index(
                Index::create()
                    .name("idx-error_event-created_at")
                    .table(ErrorEvent::Table)
                    .col(ErrorEvent::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // Makes the account-deletion anonymisation sweep cheap.
        manager
            .create_index(
                Index::create()
                    .name("idx-error_event-user_id")
                    .table(ErrorEvent::Table)
                    .col(ErrorEvent::UserId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ErrorEvent::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum ErrorEvent {
    Table,
    Id,
    IssueId,
    Source,
    Level,
    ErrorType,
    Message,
    Stack,
    Frames,
    Context,
    Release,
    Environment,
    UserId,
    UserEmail,
    ClientIp,
    CreatedAt,
}
