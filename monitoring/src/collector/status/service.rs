//! Building the status snapshot and managing what it reports on.
//!
//! Docs: docs/src/content/docs/monitoring/status-page.md

use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    DbBackend, DbErr, EntityTrait, QueryFilter, QueryOrder, Statement, Value,
};
use serde::Deserialize;
use uuid::Uuid;

use super::snapshot::{
    overall_state, DayUptime, PublicComponent, PublicIncident, PublicIncidentUpdate, PublicState,
    StatusSnapshot,
};
use crate::collector::{
    models::{project, status_component, status_incident, status_incident_update, uptime_check},
    uptime::state::CheckState,
};

/// Days of history the page shows.
const HISTORY_DAYS: i64 = 90;
/// Resolved incidents kept in the snapshot.
const RECENT_INCIDENTS: usize = 10;
/// How long a resolved incident stays on the page.
const RECENT_INCIDENT_DAYS: i64 = 14;

/// Creating or updating a component.
#[derive(Debug, Clone, Deserialize)]
pub struct UpsertComponent {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub position: Option<i32>,
    /// Follow an uptime check rather than an operator's opinion.
    #[serde(default)]
    pub auto_from_check_id: Option<Uuid>,
    #[serde(default)]
    pub manual_state: Option<String>,
}

/// Opening an incident.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenIncident {
    pub title: String,
    #[serde(default)]
    pub impact: Option<String>,
    #[serde(default)]
    pub component_ids: Vec<Uuid>,
    /// The first timeline entry.
    pub body: String,
}

/// Adding to an incident's timeline.
#[derive(Debug, Clone, Deserialize)]
pub struct AddUpdate {
    /// `investigating` | `identified` | `monitoring` | `resolved`.
    pub status: String,
    pub body: String,
}

/// Create a component.
///
/// # Errors
///
/// Returns the database error.
pub async fn create_component(
    db: &DatabaseConnection,
    project_id: Uuid,
    input: UpsertComponent,
) -> Result<status_component::Model, DbErr> {
    status_component::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        name: Set(truncate(input.name.trim(), 200)),
        description: Set(input.description.map(|d| truncate(&d, 1_000))),
        position: Set(input.position.unwrap_or(0)),
        auto_from_check_id: Set(input.auto_from_check_id),
        manual_state: Set(input
            .manual_state
            .unwrap_or_else(|| PublicState::Operational.as_str().to_string())),
        created_at: Set(Utc::now().naive_utc()),
    }
    .insert(db)
    .await
}

/// Set a component's manual state.
///
/// Ignored while the component follows a check — an operator override that a
/// probe silently contradicts would be worse than no override at all.
///
/// # Errors
///
/// Returns the database error.
pub async fn set_component_state(
    db: &DatabaseConnection,
    project_id: Uuid,
    id: Uuid,
    state: &str,
) -> Result<Option<status_component::Model>, DbErr> {
    let Some(model) = status_component::Entity::find_by_id(id)
        .filter(status_component::Column::ProjectId.eq(project_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let mut active: status_component::ActiveModel = model.into();
    active.manual_state = Set(PublicState::from_str_or_operational(state)
        .as_str()
        .to_string());
    Ok(Some(active.update(db).await?))
}

/// Delete a component.
///
/// # Errors
///
/// Returns the database error.
pub async fn delete_component(
    db: &DatabaseConnection,
    project_id: Uuid,
    id: Uuid,
) -> Result<bool, DbErr> {
    let result = status_component::Entity::delete_many()
        .filter(status_component::Column::Id.eq(id))
        .filter(status_component::Column::ProjectId.eq(project_id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Open an incident with its first timeline entry.
///
/// # Errors
///
/// Returns the database error.
pub async fn open_incident(
    db: &DatabaseConnection,
    project_id: Uuid,
    input: OpenIncident,
) -> Result<status_incident::Model, DbErr> {
    let now = Utc::now().naive_utc();
    let incident = status_incident::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        title: Set(truncate(input.title.trim(), 300)),
        status: Set("investigating".to_string()),
        impact: Set(normalize_impact(input.impact.as_deref())),
        component_ids: Set(serde_json::to_value(&input.component_ids).unwrap_or_default()),
        started_at: Set(now),
        resolved_at: Set(None),
        created_at: Set(now),
    }
    .insert(db)
    .await?;

    status_incident_update::ActiveModel {
        id: Set(Uuid::new_v4()),
        incident_id: Set(incident.id),
        status: Set("investigating".to_string()),
        body: Set(truncate(input.body.trim(), 5_000)),
        created_at: Set(now),
    }
    .insert(db)
    .await?;

    Ok(incident)
}

/// Append to an incident's timeline, moving its status.
///
/// # Errors
///
/// Returns the database error.
pub async fn add_update(
    db: &DatabaseConnection,
    project_id: Uuid,
    incident_id: Uuid,
    input: AddUpdate,
) -> Result<Option<status_incident::Model>, DbErr> {
    let Some(incident) = status_incident::Entity::find_by_id(incident_id)
        .filter(status_incident::Column::ProjectId.eq(project_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    let now = Utc::now().naive_utc();
    let status = normalize_incident_status(&input.status);

    status_incident_update::ActiveModel {
        id: Set(Uuid::new_v4()),
        incident_id: Set(incident.id),
        status: Set(status.clone()),
        body: Set(truncate(input.body.trim(), 5_000)),
        created_at: Set(now),
    }
    .insert(db)
    .await?;

    let mut active: status_incident::ActiveModel = incident.into();
    active.status = Set(status.clone());
    if status == "resolved" {
        active.resolved_at = Set(Some(now));
    }
    Ok(Some(active.update(db).await?))
}

fn normalize_impact(impact: Option<&str>) -> String {
    match impact {
        Some("critical") => "critical",
        Some("major") => "major",
        _ => "minor",
    }
    .to_string()
}

fn normalize_incident_status(status: &str) -> String {
    match status {
        "identified" => "identified",
        "monitoring" => "monitoring",
        "resolved" => "resolved",
        _ => "investigating",
    }
    .to_string()
}

/// Build the public snapshot from current state.
///
/// # Errors
///
/// Returns the database error.
pub async fn build_snapshot(
    db: &DatabaseConnection,
    project: &project::Model,
    default_name: &str,
    refresh_seconds: u64,
) -> Result<StatusSnapshot, DbErr> {
    let components = build_components(db, project.id).await?;
    let (active_incidents, recent_incidents) = build_incidents(db, project.id).await?;

    // The project's own heading, falling back to the collector-wide one so an
    // operator who never set it still gets a titled page.
    let name = match project.status_name.trim() {
        "" => default_name,
        name => name,
    };

    Ok(StatusSnapshot {
        name: name.to_string(),
        state: overall_state(&components),
        generated_at: Utc::now().naive_utc(),
        refresh_seconds,
        components,
        active_incidents,
        recent_incidents,
    })
}

async fn build_components(
    db: &DatabaseConnection,
    project_id: Uuid,
) -> Result<Vec<PublicComponent>, DbErr> {
    let rows = status_component::Entity::find()
        .filter(status_component::Column::ProjectId.eq(project_id))
        .order_by_asc(status_component::Column::Position)
        .order_by_asc(status_component::Column::Name)
        .all(db)
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (state, uptime_ratio, history) = match row.auto_from_check_id {
            Some(check_id) => component_from_check(db, check_id).await?,
            // No probe attached: the operator's word is the only source.
            None => (
                PublicState::from_str_or_operational(&row.manual_state),
                None,
                Vec::new(),
            ),
        };

        out.push(PublicComponent {
            id: row.id,
            name: row.name,
            description: row.description,
            state,
            uptime_ratio,
            history,
        });
    }
    Ok(out)
}

async fn component_from_check(
    db: &DatabaseConnection,
    check_id: Uuid,
) -> Result<(PublicState, Option<f64>, Vec<DayUptime>), DbErr> {
    let check = uptime_check::Entity::find_by_id(check_id).one(db).await?;

    let state = match check
        .as_ref()
        .map(|c| CheckState::from_str_or_unknown(&c.current_state))
    {
        Some(CheckState::Down) => PublicState::MajorOutage,
        // A check that has never reported must not be announced as an outage.
        Some(CheckState::Up | CheckState::Unknown) | None => PublicState::Operational,
    };

    let since = Utc::now().naive_utc() - Duration::days(HISTORY_DAYS);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT to_char(date_trunc('day', checked_at), 'YYYY-MM-DD') AS day,
                    count(*)::bigint                    AS total,
                    count(*) FILTER (WHERE ok)::bigint  AS ok_count
             FROM uptime_result
             WHERE check_id = $1 AND checked_at >= $2
             GROUP BY 1
             ORDER BY 1",
            vec![
                check_id.into(),
                Value::ChronoDateTime(Some(Box::new(since))),
            ],
        ))
        .await?;

    let mut total_all = 0i64;
    let mut ok_all = 0i64;
    let mut history = Vec::with_capacity(rows.len());
    for row in rows {
        let day: String = row.try_get("", "day")?;
        let total: i64 = row.try_get("", "total")?;
        let ok_count: i64 = row.try_get("", "ok_count")?;
        total_all += total;
        ok_all += ok_count;
        history.push(DayUptime {
            day,
            ratio: (total > 0).then(|| ok_count as f64 / total as f64),
        });
    }

    // Unmeasured is not the same as zero.
    let ratio = (total_all > 0).then(|| ok_all as f64 / total_all as f64);
    Ok((state, ratio, history))
}

async fn build_incidents(
    db: &DatabaseConnection,
    project_id: Uuid,
) -> Result<(Vec<PublicIncident>, Vec<PublicIncident>), DbErr> {
    let cutoff = Utc::now().naive_utc() - Duration::days(RECENT_INCIDENT_DAYS);

    let rows = status_incident::Entity::find()
        .filter(status_incident::Column::ProjectId.eq(project_id))
        .filter(
            status_incident::Column::ResolvedAt
                .is_null()
                .or(status_incident::Column::StartedAt.gte(cutoff)),
        )
        .order_by_desc(status_incident::Column::StartedAt)
        .all(db)
        .await?;

    let mut active = Vec::new();
    let mut recent = Vec::new();

    for row in rows {
        let updates = status_incident_update::Entity::find()
            .filter(status_incident_update::Column::IncidentId.eq(row.id))
            .order_by_asc(status_incident_update::Column::CreatedAt)
            .all(db)
            .await?
            .into_iter()
            .map(|u| PublicIncidentUpdate {
                status: u.status,
                body: u.body,
                created_at: u.created_at,
            })
            .collect();

        let incident = PublicIncident {
            id: row.id,
            title: row.title,
            status: row.status,
            impact: row.impact,
            started_at: row.started_at,
            resolved_at: row.resolved_at,
            updates,
        };

        if incident.resolved_at.is_none() {
            active.push(incident);
        } else if recent.len() < RECENT_INCIDENTS {
            recent.push(incident);
        }
    }

    Ok((active, recent))
}

/// List components for the operator console.
///
/// # Errors
///
/// Returns the database error.
pub async fn list_components(
    db: &DatabaseConnection,
    project_id: Uuid,
) -> Result<Vec<status_component::Model>, DbErr> {
    status_component::Entity::find()
        .filter(status_component::Column::ProjectId.eq(project_id))
        .order_by_asc(status_component::Column::Position)
        .all(db)
        .await
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_falls_back_to_minor() {
        assert_eq!(normalize_impact(Some("critical")), "critical");
        assert_eq!(normalize_impact(Some("major")), "major");
        assert_eq!(normalize_impact(Some("nonsense")), "minor");
        assert_eq!(normalize_impact(None), "minor");
    }

    #[test]
    fn incident_status_falls_back_to_investigating() {
        assert_eq!(normalize_incident_status("resolved"), "resolved");
        assert_eq!(normalize_incident_status("monitoring"), "monitoring");
        assert_eq!(normalize_incident_status("whatever"), "investigating");
    }
}
