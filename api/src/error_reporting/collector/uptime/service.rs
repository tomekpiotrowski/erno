//! Storing checks and their results.
//!
//! Docs: docs/src/content/docs/monitoring/uptime.md

use chrono::{Duration, NaiveDateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    DbBackend, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Statement, Value,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::state::{apply_probe, CheckState, ProbeOutcome};
use crate::error_reporting::collector::models::{uptime_check, uptime_result};

/// A check as the console shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckDto {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub method: String,
    pub expected_status: i32,
    pub interval_seconds: i32,
    pub enabled: bool,
    pub state: String,
    pub consecutive_failures: i32,
    pub state_changed_at: Option<NaiveDateTime>,
    pub last_checked_at: Option<NaiveDateTime>,
    /// Successful probes over total, in the requested window.
    pub uptime_ratio: Option<f64>,
    /// Median probe duration in the window.
    pub p50_ms: Option<i64>,
    /// 95th percentile probe duration in the window.
    pub p95_ms: Option<i64>,
}

/// Every check, with its recent record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckListResponse {
    pub checks: Vec<CheckDto>,
    /// Hours the ratios and percentiles cover.
    pub window_hours: i64,
}

/// What an operator posts to create or update a check.
#[derive(Debug, Clone, Deserialize)]
pub struct UpsertCheck {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub expected_status: Option<i32>,
    #[serde(default)]
    pub timeout_ms: Option<i32>,
    #[serde(default)]
    pub interval_seconds: Option<i32>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub assert_body_contains: Option<String>,
    #[serde(default)]
    pub failure_threshold: Option<i32>,
}

/// Why a check could not be saved.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    /// The submitted values do not describe a usable check.
    #[error("{0}")]
    Invalid(String),
    /// The database rejected the write.
    #[error(transparent)]
    Db(#[from] DbErr),
}

/// Create a check.
///
/// # Errors
///
/// [`CheckError::Invalid`] when the input is unusable, otherwise the database error.
pub async fn create(
    db: &DatabaseConnection,
    project_id: Uuid,
    input: UpsertCheck,
) -> Result<uptime_check::Model, CheckError> {
    let name = input.name.trim();
    let url = input.url.trim();

    if name.is_empty() {
        return Err(CheckError::Invalid("name is required".to_string()));
    }
    // A probe is an outbound request from the monitoring deployment, so the
    // target has to be something it can actually reach over HTTP.
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(CheckError::Invalid(
            "url must start with http:// or https://".to_string(),
        ));
    }

    let model = uptime_check::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        name: Set(truncate(name, 200)),
        url: Set(truncate(url, 2_000)),
        method: Set(input
            .method
            .unwrap_or_else(|| "GET".to_string())
            .to_uppercase()),
        expected_status: Set(input.expected_status.unwrap_or(200).clamp(100, 599)),
        timeout_ms: Set(input.timeout_ms.unwrap_or(10_000).clamp(100, 60_000)),
        // A floor, so a misconfigured check cannot hammer a target.
        interval_seconds: Set(input.interval_seconds.unwrap_or(60).clamp(10, 86_400)),
        enabled: Set(input.enabled.unwrap_or(true)),
        assert_body_contains: Set(input
            .assert_body_contains
            .map(|s| truncate(&s, 500))
            .filter(|s| !s.is_empty())),
        failure_threshold: Set(input.failure_threshold.unwrap_or(2).clamp(1, 10)),
        current_state: Set(CheckState::Unknown.as_str().to_string()),
        consecutive_failures: Set(0),
        state_changed_at: Set(None),
        last_checked_at: Set(None),
        created_at: Set(Utc::now().naive_utc()),
    };

    Ok(model.insert(db).await?)
}

/// Delete a check and its results.
///
/// # Errors
///
/// Returns the database error.
pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<bool, DbErr> {
    let result = uptime_check::Entity::delete_by_id(id).exec(db).await?;
    Ok(result.rows_affected > 0)
}

/// Turn a check on or off without deleting its history.
///
/// # Errors
///
/// Returns the database error.
pub async fn set_enabled(
    db: &DatabaseConnection,
    id: Uuid,
    enabled: bool,
) -> Result<Option<uptime_check::Model>, DbErr> {
    let Some(model) = uptime_check::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: uptime_check::ActiveModel = model.into();
    active.enabled = Set(enabled);
    Ok(Some(active.update(db).await?))
}

/// List checks with their record over a window.
///
/// # Errors
///
/// Returns the database error.
pub async fn list(db: &DatabaseConnection, window_hours: i64) -> Result<CheckListResponse, DbErr> {
    let window_hours = window_hours.clamp(1, 24 * 365);
    let since = Utc::now().naive_utc() - Duration::hours(window_hours);

    let checks = uptime_check::Entity::find()
        .order_by_asc(uptime_check::Column::Name)
        .all(db)
        .await?;

    let mut out = Vec::with_capacity(checks.len());
    for check in checks {
        let stats = window_stats(db, check.id, since).await?;
        out.push(CheckDto {
            id: check.id,
            name: check.name,
            url: check.url,
            method: check.method,
            expected_status: check.expected_status,
            interval_seconds: check.interval_seconds,
            enabled: check.enabled,
            state: check.current_state,
            consecutive_failures: check.consecutive_failures,
            state_changed_at: check.state_changed_at,
            last_checked_at: check.last_checked_at,
            uptime_ratio: stats.uptime_ratio,
            p50_ms: stats.p50_ms,
            p95_ms: stats.p95_ms,
        });
    }

    Ok(CheckListResponse {
        checks: out,
        window_hours,
    })
}

struct WindowStats {
    uptime_ratio: Option<f64>,
    p50_ms: Option<i64>,
    p95_ms: Option<i64>,
}

async fn window_stats(
    db: &DatabaseConnection,
    check_id: Uuid,
    since: NaiveDateTime,
) -> Result<WindowStats, DbErr> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT
                 count(*)::bigint                                   AS total,
                 count(*) FILTER (WHERE ok)::bigint                 AS ok_count,
                 percentile_disc(0.5) WITHIN GROUP (ORDER BY duration_ms)::bigint  AS p50,
                 percentile_disc(0.95) WITHIN GROUP (ORDER BY duration_ms)::bigint AS p95
             FROM uptime_result
             WHERE check_id = $1 AND checked_at >= $2",
            vec![check_id.into(), since.into()],
        ))
        .await?;

    let Some(row) = row else {
        return Ok(WindowStats {
            uptime_ratio: None,
            p50_ms: None,
            p95_ms: None,
        });
    };

    let total: i64 = row.try_get("", "total")?;
    let ok_count: i64 = row.try_get("", "ok_count")?;

    Ok(WindowStats {
        // No probes yet is unknown, not zero — an empty check must not look
        // like a broken one.
        uptime_ratio: (total > 0).then(|| ok_count as f64 / total as f64),
        p50_ms: row.try_get("", "p50").ok(),
        p95_ms: row.try_get("", "p95").ok(),
    })
}

/// Checks that are enabled and due to run.
///
/// # Errors
///
/// Returns the database error.
pub async fn due(db: &DatabaseConnection) -> Result<Vec<uptime_check::Model>, DbErr> {
    let now = Utc::now().naive_utc();
    uptime_check::Entity::find()
        .filter(uptime_check::Column::Enabled.eq(true))
        .filter(
            uptime_check::Column::LastCheckedAt
                .is_null()
                .or(Expr::cust_with_values(
                    "last_checked_at < $1::timestamp - (interval_seconds || ' seconds')::interval",
                    [Value::ChronoDateTime(Some(Box::new(now)))],
                )),
        )
        .limit(100)
        .all(db)
        .await
}

/// Record a probe and move the check's state.
///
/// Returns whether the state changed, which is the only moment worth telling
/// anyone about.
///
/// # Errors
///
/// Returns the database error.
pub async fn record_probe(
    db: &DatabaseConnection,
    check: &uptime_check::Model,
    outcome: &ProbeOutcome,
) -> Result<bool, DbErr> {
    let now = Utc::now().naive_utc();

    uptime_result::ActiveModel {
        id: Set(Uuid::new_v4()),
        check_id: Set(check.id),
        ok: Set(outcome.ok),
        status_code: Set(outcome.status_code.map(i32::from)),
        duration_ms: Set(i32::try_from(outcome.duration_ms).unwrap_or(i32::MAX)),
        error: Set(outcome.error.as_deref().map(|e| truncate(e, 500))),
        checked_at: Set(now),
    }
    .insert(db)
    .await?;

    let transition = apply_probe(
        CheckState::from_str_or_unknown(&check.current_state),
        check.consecutive_failures,
        check.failure_threshold,
        outcome.ok,
    );

    let mut active: uptime_check::ActiveModel = check.clone().into();
    active.current_state = Set(transition.state.as_str().to_string());
    active.consecutive_failures = Set(transition.consecutive_failures);
    active.last_checked_at = Set(Some(now));
    if transition.changed {
        active.state_changed_at = Set(Some(now));
    }
    active.update(db).await?;

    Ok(transition.changed)
}

/// Drop probe results older than the retention window.
///
/// Raw results are high volume and are only interesting while recent; the
/// ratios above are what matter after that.
///
/// # Errors
///
/// Returns the database error.
pub async fn prune_results(db: &DatabaseConnection, retention_days: i64) -> Result<u64, DbErr> {
    let cutoff = Utc::now().naive_utc() - Duration::days(retention_days.max(1));
    let result = uptime_result::Entity::delete_many()
        .filter(uptime_result::Column::CheckedAt.lt(cutoff))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}

use sea_orm::sea_query::Expr;
