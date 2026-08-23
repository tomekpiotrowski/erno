//! Widen `alert_rule.selector` so it can hold a PromQL expression.
//!
//! The column was sized for a check id or a gauge name. A PromQL query is
//! routinely longer, and the failure mode of truncating one is bad: the shorter
//! string usually still parses, so the rule evaluates something other than what
//! the operator wrote, silently.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AlertRule::Table)
                    .modify_column(ColumnDef::new(AlertRule::Selector).text().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Narrowing would fail on any row already holding a long expression, so
        // this is deliberately a no-op rather than a data-losing truncation.
        let _ = manager;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AlertRule {
    Table,
    Selector,
}
