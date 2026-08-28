//! Docs: docs/src/content/docs/monitoring/status-page.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A publicly communicated incident.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "status_incident")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    /// `investigating` | `identified` | `monitoring` | `resolved`.
    pub status: String,
    /// `minor` | `major` | `critical`.
    pub impact: String,
    /// Affected component ids.
    #[sea_orm(column_type = "JsonBinary")]
    pub component_ids: Json,
    pub started_at: NaiveDateTime,
    pub resolved_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::status_incident_update::Entity")]
    Updates,
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Project,
}

impl Related<super::status_incident_update::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Updates.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
