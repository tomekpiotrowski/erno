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
                    .table(Users::Table)
                    .if_not_exists()
                    .col(
                        uuid(Users::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(string(Users::Email).not_null().unique_key())
                    .col(string(Users::PasswordHash).not_null())
                    .col(ColumnDef::new(Users::EmailVerifiedAt).timestamp().null())
                    .col(
                        timestamp(Users::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .col(
                        timestamp(Users::UpdatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TRIGGER update_users_updated_at
                  BEFORE UPDATE ON users
                  FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum Users {
    Table,
    Id,
    Email,
    PasswordHash,
    EmailVerifiedAt,
    CreatedAt,
    UpdatedAt,
}
