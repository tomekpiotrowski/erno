//! Docs: docs/src/content/docs/monitoring/subsystem-health.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// The latest health reading from one application instance.
///
/// One row per instance, replaced on every heartbeat: this answers "is it
/// healthy right now", and a heartbeat that stops arriving is itself the
/// signal that matters most.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "app_health")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub instance: String,
    pub environment: String,
    pub release: Option<String>,
    pub reported_at: NaiveDateTime,
    #[sea_orm(column_type = "JsonBinary")]
    pub payload: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
