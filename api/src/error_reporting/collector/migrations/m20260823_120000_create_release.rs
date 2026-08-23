//! Docs: docs/src/content/docs/monitoring/releases.md
use sea_orm_migration::{
    prelude::*,
    schema::{string, string_null, timestamp, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Release::Table)
                    .if_not_exists()
                    .col(
                        uuid(Release::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(Release::Version).not_null())
                    .col(string(Release::Environment).not_null())
                    .col(string_null(Release::CommitSha))
                    .col(string_null(Release::Source))
                    .col(timestamp(Release::DeployedAt).not_null())
                    .col(
                        timestamp(Release::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        // A deploy is idempotent: re-running a pipeline for the same version and
        // environment must update the existing row, not create a duplicate.
        manager
            .create_index(
                Index::create()
                    .name("uq-release-version_environment")
                    .table(Release::Table)
                    .col(Release::Version)
                    .col(Release::Environment)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-release-deployed_at")
                    .table(Release::Table)
                    .col(Release::DeployedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Release::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Release {
    Table,
    Id,
    Version,
    Environment,
    CommitSha,
    Source,
    DeployedAt,
    CreatedAt,
}
