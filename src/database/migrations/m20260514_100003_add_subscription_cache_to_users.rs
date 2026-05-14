use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(ColumnDef::new(Users::SubscriptionId).uuid().null())
                    .add_column(ColumnDef::new(Users::SubscriptionType).string().null())
                    .add_column(ColumnDef::new(Users::SubscriptionPlan).string().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .drop_column(Users::SubscriptionId)
                    .drop_column(Users::SubscriptionType)
                    .drop_column(Users::SubscriptionPlan)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum Users {
    Table,
    SubscriptionId,
    SubscriptionType,
    SubscriptionPlan,
}
