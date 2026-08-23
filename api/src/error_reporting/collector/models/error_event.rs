//! Docs: docs/src/content/docs/monitoring/error-reporting.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;

/// One stored occurrence of an issue.
///
/// Not every reported occurrence becomes a row: the per-flush burst cap stores
/// a bounded number per fingerprint while still counting them all against
/// `error_issue.times_seen`.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "error_event")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub issue_id: Uuid,
    pub source: String,
    pub level: String,
    pub error_type: String,
    #[sea_orm(column_type = "Text")]
    pub message: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub stack: Option<String>,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub frames: Option<Json>,
    #[sea_orm(column_type = "JsonBinary")]
    pub context: Json,
    pub release: Option<String>,
    pub environment: Option<String>,
    /// Bare uuid, no foreign key — the users table lives in the application's
    /// database. Nulled in place when the account is deleted.
    pub user_id: Option<Uuid>,
    /// Denormalised by the reporting app, since the collector cannot look
    /// users up. Cleared alongside `user_id` on account deletion.
    pub user_email: Option<String>,
    pub client_ip: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::error_issue::Entity",
        from = "Column::IssueId",
        to = "super::error_issue::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    ErrorIssue,
}

impl Related<super::error_issue::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ErrorIssue.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
