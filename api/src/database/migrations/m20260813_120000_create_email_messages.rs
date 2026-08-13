use sea_orm_migration::{
    prelude::*,
    schema::{string, string_null, timestamp, timestamp_null, uuid, uuid_null},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EmailMessages::Table)
                    .if_not_exists()
                    .col(
                        uuid(EmailMessages::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(EmailMessages::To).not_null())
                    .col(string(EmailMessages::From).not_null())
                    .col(string(EmailMessages::Subject).not_null())
                    .col(string_null(EmailMessages::Template))
                    .col(uuid_null(EmailMessages::UserId))
                    .col(uuid_null(EmailMessages::JobId))
                    .col(string(EmailMessages::Status).not_null())
                    .col(string_null(EmailMessages::Error))
                    .col(timestamp_null(EmailMessages::SentAt))
                    .col(
                        timestamp(EmailMessages::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_email_messages_user_id")
                            .from(EmailMessages::Table, EmailMessages::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::SetNull)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_email_messages_job_id")
                            .from(EmailMessages::Table, EmailMessages::JobId)
                            .to(Job::Table, Job::Id)
                            .on_delete(ForeignKeyAction::SetNull)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-email_messages-created_at")
                    .table(EmailMessages::Table)
                    .col(EmailMessages::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-email_messages-to")
                    .table(EmailMessages::Table)
                    .col(EmailMessages::To)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(EmailMessages::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum EmailMessages {
    Table,
    Id,
    To,
    From,
    Subject,
    Template,
    UserId,
    JobId,
    Status,
    Error,
    SentAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Job {
    Table,
    Id,
}
