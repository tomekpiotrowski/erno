use sea_orm_migration::{
    prelude::*,
    schema::{json_binary, string, timestamp, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Job::Table)
                    .if_not_exists()
                    .col(
                        uuid(Job::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(
                        timestamp(Job::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .col(
                        timestamp(Job::UpdatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .col(string(Job::Type).not_null())
                    .col(json_binary(Job::Arguments).not_null())
                    .col(
                        ColumnDef::new(Job::Status)
                            .string_len(32)
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(Job::RetryCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Job::NextExecutionAt).timestamp().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(JobExecution::Table)
                    .if_not_exists()
                    .col(
                        uuid(JobExecution::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(uuid(JobExecution::JobId).not_null())
                    .col(
                        ColumnDef::new(JobExecution::Result)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(timestamp(JobExecution::StartedAt).not_null())
                    .col(timestamp(JobExecution::FinishedAt).not_null())
                    .col(
                        ColumnDef::new(JobExecution::ExecutionTimeMs)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(JobExecution::FailureReason).string().null())
                    .col(
                        timestamp(JobExecution::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-job_execution-job_id")
                            .from(JobExecution::Table, JobExecution::JobId)
                            .to(Job::Table, Job::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-job_execution-job_id")
                    .table(JobExecution::Table)
                    .col(JobExecution::JobId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-job_execution-created_at")
                    .table(JobExecution::Table)
                    .col(JobExecution::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                r"
                CREATE TRIGGER update_job_updated_at
                    BEFORE UPDATE ON job
                    FOR EACH ROW
                    EXECUTE FUNCTION update_updated_at_column();
                ",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                r"
                CREATE OR REPLACE FUNCTION notify_job_insert()
                RETURNS trigger AS $$
                BEGIN
                    PERFORM pg_notify('job_new', NEW.id::text);
                    RETURN NEW;
                END;
                $$ LANGUAGE plpgsql;

                CREATE TRIGGER job_insert_notify
                    AFTER INSERT ON job
                    FOR EACH ROW
                    EXECUTE FUNCTION notify_job_insert();
                ",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r"
                DROP TRIGGER IF EXISTS job_insert_notify ON job;
                DROP FUNCTION IF EXISTS notify_job_insert();
                DROP TRIGGER IF EXISTS update_job_updated_at ON job;
                ",
            )
            .await?;

        manager
            .drop_table(Table::drop().table(JobExecution::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Job::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Job {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
    Type,
    Arguments,
    Status,
    RetryCount,
    NextExecutionAt,
}

#[derive(DeriveIden)]
enum JobExecution {
    Table,
    Id,
    JobId,
    Result,
    StartedAt,
    FinishedAt,
    ExecutionTimeMs,
    FailureReason,
    CreatedAt,
}
