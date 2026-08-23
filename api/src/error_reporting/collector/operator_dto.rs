//! Operator-facing response shapes.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! `snake_case` throughout, matching the rest of Erno's admin DTOs — the UI
//! consumes these field names verbatim rather than converting.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One row of the issues list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueSummary {
    pub id: Uuid,
    pub fingerprint: String,
    pub source: String,
    pub error_type: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: String,
    pub status: String,
    /// Lifetime occurrence count. A floor, not an exact number, whenever
    /// reports were shed under load — the UI shows it beside `stored_events`.
    pub times_seen: i64,
    pub first_seen: NaiveDateTime,
    pub last_seen: NaiveDateTime,
    pub first_release: Option<String>,
    pub last_release: Option<String>,
    pub environment: Option<String>,
}

/// A paginated issues list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueListResponse {
    pub issues: Vec<IssueSummary>,
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
}

/// One stored occurrence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDto {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub source: String,
    pub level: String,
    pub error_type: String,
    pub message: String,
    pub stack: Option<String>,
    pub frames: Option<serde_json::Value>,
    pub context: serde_json::Value,
    pub release: Option<String>,
    pub environment: Option<String>,
    pub user_id: Option<Uuid>,
    pub user_email: Option<String>,
    pub created_at: NaiveDateTime,
}

/// Everything the issue detail screen needs in one round trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueDetail {
    pub issue: IssueSummary,
    /// How many event rows actually exist, as opposed to `times_seen`.
    pub stored_events: i64,
    pub latest_event: Option<EventDto>,
    pub events: Vec<EventDto>,
}

/// A paginated event list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventListResponse {
    pub events: Vec<EventDto>,
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
}

/// One bucket of a time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesPoint {
    pub t: NaiveDateTime,
    pub count: i64,
}

/// A zero-filled time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesResponse {
    pub points: Vec<SeriesPoint>,
    /// `minute`, `hour`, or `day` — chosen server-side from the window.
    pub bucket: String,
}

/// Counts for the list header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueCounts {
    pub unresolved: i64,
    pub resolved: i64,
    pub ignored: i64,
}

/// Filters accepted by the issues list.
#[derive(Debug, Clone, Deserialize)]
pub struct IssueQuery {
    /// `unresolved` (default), `resolved`, `ignored`, or `all`.
    #[serde(default)]
    pub status: Option<String>,
    /// `api`, `app`, `admin`, or absent for all.
    #[serde(default)]
    pub source: Option<String>,
    /// Case-insensitive substring over title, type, and culprit.
    #[serde(default)]
    pub q: Option<String>,
    /// Restrict to a release.
    #[serde(default)]
    pub release: Option<String>,
    /// Look-back window in hours.
    #[serde(default)]
    pub hours: Option<i64>,
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub per_page: Option<u64>,
}

/// Pagination for the events list.
#[derive(Debug, Clone, Deserialize)]
pub struct EventQuery {
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub per_page: Option<u64>,
}

/// Window for a time series.
#[derive(Debug, Clone, Deserialize)]
pub struct SeriesQuery {
    #[serde(default)]
    pub hours: Option<i64>,
    #[serde(default)]
    pub source: Option<String>,
}
