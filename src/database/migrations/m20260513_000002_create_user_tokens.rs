use sea_orm::{DbBackend, Schema};
use sea_orm_migration::{
    prelude::*,
    schema::{string, timestamp, uuid},
};

use crate::database::models::user_token_type::UserTokenType;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let schema = Schema::new(DbBackend::Postgres);
        manager
            .create_type(schema.create_enum_from_active_enum::<UserTokenType>())
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UserTokens::Table)
                    .if_not_exists()
                    .col(
                        uuid(UserTokens::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(uuid(UserTokens::UserId).not_null())
                    .col(
                        ColumnDef::new(UserTokens::TokenType)
                            .custom(Alias::new("user_token_type"))
                            .not_null(),
                    )
                    .col(string(UserTokens::TokenHash).not_null())
                    .col(timestamp(UserTokens::ExpiresAt).not_null())
                    .col(
                        timestamp(UserTokens::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_user_tokens_user_id")
                            .from(UserTokens::Table, UserTokens::UserId)
                            .to(Alias::new("users"), Alias::new("id"))
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(UserTokens::Table).to_owned())
            .await?;
        manager
            .drop_type(
                extension::postgres::TypeDropStatement::new()
                    .name(Alias::new("user_token_type"))
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
pub enum UserTokens {
    Table,
    Id,
    UserId,
    TokenType,
    TokenHash,
    ExpiresAt,
    CreatedAt,
}
