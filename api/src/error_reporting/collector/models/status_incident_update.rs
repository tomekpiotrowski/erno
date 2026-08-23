//! Docs: docs/src/content/docs/monitoring/status-page.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// One entry in an incident's timeline.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "status_incident_update")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub incident_id: Uuid,
    pub status: String,
    #[sea_orm(column_type = "Text")]
    pub body: String,
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::status_incident::Entity",
        from = "Column::IncidentId",
        to = "super::status_incident::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Incident,
}

impl Related<super::status_incident::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Incident.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
