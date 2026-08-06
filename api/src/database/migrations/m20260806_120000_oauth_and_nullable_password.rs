use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // OAuth-only users have no password.
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE users ALTER COLUMN password_hash DROP NOT NULL",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TYPE user_token_type ADD VALUE IF NOT EXISTS 'oauth_exchange'",
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(OauthIdentities::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(OauthIdentities::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(OauthIdentities::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(OauthIdentities::Provider)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OauthIdentities::ProviderSubject)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(OauthIdentities::Email).string().null())
                    .col(
                        ColumnDef::new(OauthIdentities::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .col(
                        ColumnDef::new(OauthIdentities::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth_identities_user_id")
                            .from(OauthIdentities::Table, OauthIdentities::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_oauth_identities_provider_subject")
                    .table(OauthIdentities::Table)
                    .col(OauthIdentities::Provider)
                    .col(OauthIdentities::ProviderSubject)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_oauth_identities_user_id")
                    .table(OauthIdentities::Table)
                    .col(OauthIdentities::UserId)
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TRIGGER update_oauth_identities_updated_at
                  BEFORE UPDATE ON oauth_identities
                  FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TRIGGER IF EXISTS update_oauth_identities_updated_at ON oauth_identities")
            .await?;
        manager
            .drop_table(Table::drop().table(OauthIdentities::Table).to_owned())
            .await?;
        // Cannot re-add NOT NULL if any nulls exist; leave password_hash nullable.
        // Cannot remove enum values in Postgres without type rebuild.
        Ok(())
    }
}

#[derive(Iden)]
enum Users {
    Table,
    Id,
}

#[derive(Iden)]
enum OauthIdentities {
    Table,
    Id,
    UserId,
    Provider,
    ProviderSubject,
    Email,
    CreatedAt,
    UpdatedAt,
}
