//! Docs: docs/src/content/docs/monitoring/uptime.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// One probe attempt.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "uptime_result")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub check_id: Uuid,
    pub ok: bool,
    pub status_code: Option<i32>,
    pub duration_ms: i32,
    pub error: Option<String>,
    pub checked_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::uptime_check::Entity",
        from = "Column::CheckId",
        to = "super::uptime_check::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    UptimeCheck,
}

impl Related<super::uptime_check::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UptimeCheck.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
