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
                    .table(Files::Table)
                    .if_not_exists()
                    .col(
                        uuid(Files::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(Files::Key).not_null().unique_key())
                    .col(string(Files::Filename).not_null())
                    .col(string_null(Files::ContentType))
                    .col(ColumnDef::new(Files::ByteSize).big_integer().not_null())
                    .col(string(Files::Checksum).not_null())
                    .col(
                        timestamp(Files::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .col(
                        timestamp(Files::UpdatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TRIGGER update_files_updated_at
                  BEFORE UPDATE ON files
                  FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TRIGGER IF EXISTS update_files_updated_at ON files;",
            )
            .await?;

        manager
            .drop_table(Table::drop().table(Files::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum Files {
    Table,
    Id,
    Key,
    Filename,
    ContentType,
    ByteSize,
    Checksum,
    CreatedAt,
    UpdatedAt,
}
