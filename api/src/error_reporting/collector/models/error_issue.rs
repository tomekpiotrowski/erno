//! Docs: docs/src/content/docs/monitoring/error-reporting.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;

/// One row per distinct fingerprint.
///
/// `times_seen` is a lifetime counter and is never decremented — not by
/// retention, not by the per-issue cap. It is also a *floor* rather than an
/// exact count whenever reports were shed under load, which is why the UI
/// shows occurrences and stored events as separate numbers.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "error_issue")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub fingerprint: String,
    pub source: String,
    pub error_type: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: String,
    pub status: String,
    pub times_seen: i64,
    pub first_seen: NaiveDateTime,
    pub last_seen: NaiveDateTime,
    /// Set on insert only, so it stays the release the issue was born in.
    pub first_release: Option<String>,
    pub last_release: Option<String>,
    pub environment: Option<String>,
    pub resolved_at: Option<NaiveDateTime>,
    /// Idempotency guard for the new-issue alert: set once the mail goes out,
    /// so a restarted or parallel writer will not alert again.
    pub alert_sent_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::error_event::Entity")]
    ErrorEvent,
}

impl Related<super::error_event::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ErrorEvent.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// Triage state of an issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueStatus {
    /// Needs attention.
    Unresolved,
    /// Marked fixed. Recurrence in a later release reopens it.
    Resolved,
    /// Deliberately muted; recurrence does **not** reopen it.
    Ignored,
}

impl IssueStatus {
    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::Resolved => "resolved",
            Self::Ignored => "ignored",
        }
    }

    /// Parse from the database/wire representation.
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "unresolved" => Some(Self::Unresolved),
            "resolved" => Some(Self::Resolved),
            "ignored" => Some(Self::Ignored),
            _ => None,
        }
    }
}

impl std::fmt::Display for IssueStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
