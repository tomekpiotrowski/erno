//! Docs: docs/src/content/docs/monitoring/alerts.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A condition an operator wants to be told about.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "alert_rule")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub name: String,
    pub enabled: bool,
    /// `errors` | `uptime` | `subsystem`.
    pub source: String,
    /// Narrows the source — an error level, a check name, and so on.
    pub selector: String,
    pub comparator: String,
    pub threshold: f64,
    /// Look-back window for sources that count things.
    pub window_seconds: i64,
    /// How long a breach must persist before it is believed.
    pub for_seconds: i64,
    /// How often to repeat while still firing. Zero means notify once.
    pub repeat_seconds: i64,
    pub severity: String,
    pub notify_email: Option<String>,
    pub notify_webhook: Option<String>,
    /// Suppress notifications until this time. State still tracks reality.
    pub silence_until: Option<NaiveDateTime>,
    pub state: String,
    pub state_since: Option<NaiveDateTime>,
    pub last_notified_at: Option<NaiveDateTime>,
    pub last_evaluated_at: Option<NaiveDateTime>,
    /// Rendered for display; stored as text so the console never has to guess
    /// how to format it.
    pub last_value: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
