use sea_orm_migration::{
    prelude::*,
    schema::{double, string, string_null, timestamp, uuid},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(StatSnapshot::Table)
                    .if_not_exists()
                    .col(
                        uuid(StatSnapshot::Id)
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(timestamp(StatSnapshot::CapturedAt).not_null())
                    .col(string(StatSnapshot::Metric).not_null())
                    .col(string_null(StatSnapshot::Dimension))
                    .col(double(StatSnapshot::Value))
                    .col(
                        timestamp(StatSnapshot::CreatedAt)
                            .not_null()
                            .default(Expr::cust("CURRENT_TIMESTAMP")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-stat_snapshot-metric-dimension-captured_at")
                    .table(StatSnapshot::Table)
                    .col(StatSnapshot::Metric)
                    .col(StatSnapshot::Dimension)
                    .col(StatSnapshot::CapturedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(StatSnapshot::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum StatSnapshot {
    Table,
    Id,
    CapturedAt,
    Metric,
    Dimension,
    Value,
    CreatedAt,
}
