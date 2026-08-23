//! Docs: docs/src/content/docs/monitoring/status-page.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// Something the public status page reports on.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "status_component")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub position: i32,
    /// When set, state follows this uptime check instead of `manual_state`.
    pub auto_from_check_id: Option<Uuid>,
    /// Operator override, used when no check is attached.
    pub manual_state: String,
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
