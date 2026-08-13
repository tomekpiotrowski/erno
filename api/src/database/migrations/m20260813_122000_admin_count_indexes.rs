use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx-users-subscription_type")
                    .table(Users::Table)
                    .col(Users::SubscriptionType)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-users-last_active_at")
                    .table(Users::Table)
                    .col(Users::LastActiveAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-job-status")
                    .table(Job::Table)
                    .col(Job::Status)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-job-type-status")
                    .table(Job::Table)
                    .col(Job::Type)
                    .col(Job::Status)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-job_execution-finished_at")
                    .table(JobExecution::Table)
                    .col(JobExecution::FinishedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx-job_execution-finished_at")
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(Index::drop().name("idx-job-type-status").to_owned())
            .await?;
        manager
            .drop_index(Index::drop().name("idx-job-status").to_owned())
            .await?;
        manager
            .drop_index(Index::drop().name("idx-users-last_active_at").to_owned())
            .await?;
        manager
            .drop_index(Index::drop().name("idx-users-subscription_type").to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    SubscriptionType,
    LastActiveAt,
}

#[derive(DeriveIden)]
enum Job {
    Table,
    Type,
    Status,
}

#[derive(DeriveIden)]
enum JobExecution {
    Table,
    FinishedAt,
}
