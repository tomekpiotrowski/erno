//! Docs: docs/src/content/docs/monitoring/uptime.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A synthetic probe against a public endpoint.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "uptime_check")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub url: String,
    pub method: String,
    pub expected_status: i32,
    pub timeout_ms: i32,
    pub interval_seconds: i32,
    pub enabled: bool,
    /// Optional substring the response body must contain — catches the case
    /// where a server returns 200 while serving an error page.
    pub assert_body_contains: Option<String>,
    /// Consecutive failures before the check is called down.
    pub failure_threshold: i32,
    pub current_state: String,
    pub consecutive_failures: i32,
    pub state_changed_at: Option<NaiveDateTime>,
    pub last_checked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::uptime_result::Entity")]
    UptimeResult,
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Project,
}

impl Related<super::uptime_result::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UptimeResult.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
