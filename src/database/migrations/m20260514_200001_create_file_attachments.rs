use sea_orm_migration::{
    prelude::*,
    schema::{string, timestamp, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(FileAttachments::Table)
                    .if_not_exists()
                    .col(
                        uuid(FileAttachments::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(FileAttachments::Name).not_null())
                    .col(string(FileAttachments::RecordType).not_null())
                    .col(uuid(FileAttachments::RecordId).not_null())
                    .col(uuid(FileAttachments::FileId).not_null())
                    .col(
                        timestamp(FileAttachments::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .col(
                        timestamp(FileAttachments::UpdatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_file_attachments_file_id")
                            .from(FileAttachments::Table, FileAttachments::FileId)
                            .to(Alias::new("files"), Alias::new("id"))
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(FileAttachments::Table)
                    .col(FileAttachments::RecordType)
                    .col(FileAttachments::RecordId)
                    .col(FileAttachments::Name)
                    .name("idx_file_attachments_record")
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TRIGGER update_file_attachments_updated_at
                  BEFORE UPDATE ON file_attachments
                  FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TRIGGER IF EXISTS update_file_attachments_updated_at ON file_attachments;",
            )
            .await?;

        manager
            .drop_table(Table::drop().table(FileAttachments::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum FileAttachments {
    Table,
    Id,
    Name,
    RecordType,
    RecordId,
    FileId,
    CreatedAt,
    UpdatedAt,
}
