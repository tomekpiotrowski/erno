//! Docs: docs/src/content/docs/monitoring/subsystem-health.md
use sea_orm_migration::{
    prelude::*,
    schema::{json_binary, string, string_null, timestamp, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AppHealth::Table)
                    .if_not_exists()
                    .col(
                        uuid(AppHealth::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(AppHealth::Instance).not_null())
                    .col(string(AppHealth::Environment).not_null())
                    .col(string_null(AppHealth::Release))
                    .col(timestamp(AppHealth::ReportedAt).not_null())
                    .col(json_binary(AppHealth::Payload).not_null())
                    .to_owned(),
            )
            .await?;

        // Only the latest reading per instance is kept — this is a liveness
        // view, not a time series. Prometheus is the right place for history.
        manager
            .create_index(
                Index::create()
                    .name("uq-app_health-instance")
                    .table(AppHealth::Table)
                    .col(AppHealth::Instance)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AppHealth::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum AppHealth {
    Table,
    Id,
    Instance,
    Environment,
    Release,
    ReportedAt,
    Payload,
}
