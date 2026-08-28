//! Storing and judging application health readings.
//!
//! Docs: docs/src/content/docs/monitoring/subsystem-health.md

use chrono::{NaiveDateTime, Utc};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::health::snapshot::{HealthSnapshot, HealthState, HealthThresholds, SubsystemStatus};

use super::models::app_health;

/// One instance as the console shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceHealth {
    pub instance: String,
    pub environment: String,
    pub release: Option<String>,
    pub reported_at: NaiveDateTime,
    /// Seconds since the last heartbeat.
    pub age_seconds: i64,
    /// Whether the heartbeat itself has gone quiet.
    pub stale: bool,
    /// Worst state across this instance's subsystems.
    pub state: HealthState,
    /// Per-subsystem verdicts.
    pub subsystems: Vec<SubsystemStatus>,
}

/// Everything the System page needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub instances: Vec<InstanceHealth>,
    /// Worst state across every instance.
    pub state: HealthState,
}

/// Record a heartbeat, replacing this instance's previous reading.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the write fails.
pub async fn record(
    db: &DatabaseConnection,
    project_id: Uuid,
    snapshot: &HealthSnapshot,
) -> Result<(), DbErr> {
    use sea_orm::sea_query::OnConflict;

    let payload = serde_json::to_value(snapshot)
        .map_err(|e| DbErr::Custom(format!("could not serialise health snapshot: {e}")))?;

    let model = app_health::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        instance: Set(truncate(&snapshot.instance, 200)),
        environment: Set(truncate(&snapshot.environment, 100)),
        release: Set(snapshot.release.as_deref().map(|r| truncate(r, 200))),
        reported_at: Set(snapshot.reported_at),
        payload: Set(payload),
    };

    app_health::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([app_health::Column::ProjectId, app_health::Column::Instance])
                .update_columns([
                    app_health::Column::Environment,
                    app_health::Column::Release,
                    app_health::Column::ReportedAt,
                    app_health::Column::Payload,
                ])
                .to_owned(),
        )
        .exec(db)
        .await?;

    Ok(())
}

/// Judge every known instance.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the query fails.
pub async fn list(
    db: &DatabaseConnection,
    project_id: Uuid,
    thresholds: &HealthThresholds,
) -> Result<HealthResponse, DbErr> {
    let rows = app_health::Entity::find()
        .filter(app_health::Column::ProjectId.eq(project_id))
        .order_by_asc(app_health::Column::Instance)
        .all(db)
        .await?;

    let now = Utc::now().naive_utc();
    let mut instances = Vec::with_capacity(rows.len());
    let mut overall = HealthState::Ok;

    for row in rows {
        let age_seconds = (now - row.reported_at).num_seconds().max(0);
        let stale = age_seconds >= thresholds.heartbeat_stale_seconds;

        let snapshot: Option<HealthSnapshot> = serde_json::from_value(row.payload).ok();

        // A heartbeat that stopped outranks whatever the last reading said —
        // the numbers are describing a moment that has passed.
        let (state, subsystems) = match (&snapshot, stale) {
            (_, true) => (
                HealthState::Down,
                vec![SubsystemStatus {
                    name: "heartbeat".to_string(),
                    state: HealthState::Down,
                    detail: format!(
                        "no reading for {}",
                        crate::health::snapshot::humanize(age_seconds)
                    ),
                }],
            ),
            (Some(snapshot), false) => {
                (snapshot.overall(thresholds), snapshot.evaluate(thresholds))
            }
            (None, false) => (
                HealthState::Degraded,
                vec![SubsystemStatus {
                    name: "heartbeat".to_string(),
                    state: HealthState::Degraded,
                    detail: "reading could not be read; the reporter may be a newer version"
                        .to_string(),
                }],
            ),
        };

        overall = overall.worst(state);
        instances.push(InstanceHealth {
            instance: row.instance,
            environment: row.environment,
            release: row.release,
            reported_at: row.reported_at,
            age_seconds,
            stale,
            state,
            subsystems,
        });
    }

    Ok(HealthResponse {
        instances,
        state: overall,
    })
}

/// Forget instances that have not reported in a long time.
///
/// Without this, every replica a deployment has ever had accumulates in the
/// console forever, and a rolling deploy quietly doubles the list.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the delete fails.
pub async fn forget_stale(db: &DatabaseConnection, older_than_seconds: i64) -> Result<u64, DbErr> {
    use sea_orm::{ColumnTrait, QueryFilter};

    let cutoff = Utc::now().naive_utc() - chrono::Duration::seconds(older_than_seconds);
    let result = app_health::Entity::delete_many()
        .filter(app_health::Column::ReportedAt.lt(cutoff))
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
