//! Docs: docs/src/content/docs/monitoring/error-reporting.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;

/// One Erno application watched by this collector.
///
/// Ingest tokens are stored as SHA-256 hex. Plaintext is returned only at
/// create and rotate. Not `Serialize`: GET uses `ProjectDto`, never this model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "project")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub server_token_hash: String,
    pub browser_token_hash: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub cors_origins: Json,
    pub scrape_target: String,
    pub scrape_scheme: String,
    pub scrape_metrics_token: String,
    pub event_retention_days: Option<i64>,
    pub issue_retention_days: Option<i64>,
    pub max_events_per_issue: Option<i64>,
    pub status_enabled: bool,
    pub status_name: String,
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
