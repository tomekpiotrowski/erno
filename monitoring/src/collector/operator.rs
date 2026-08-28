//! Operator endpoints for the monitoring console.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Guarded by HTTP Basic auth via [`erno::admin::auth::verify_admin_basic_auth`]
//! — the same check the admin console uses, called through a middleware rather
//! than the `AdminAuth` extractor because collector handlers carry a composite
//! state. Deliberately independent of the application's auth service, which may
//! be exactly what is broken when an operator needs this screen.

use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use uuid::Uuid;

use erno::admin::auth::verify_admin_basic_auth;

use super::{
    models::IssueStatus,
    operator_dto::{EventQuery, IssueQuery, SeriesQuery},
    projects,
    releases::{self, RecordRelease, ReleaseQuery},
    service::{self, IssueFilters},
    state::CollectorState,
};

/// Resolve the `{slug}` of a nested operator route to a project id.
///
/// Every query below then filters on that id, so a row belonging to another
/// project reads as missing rather than as somebody else's data. An unknown
/// slug is a 404, the same answer as an unknown id.
async fn project_id_for(db: &sea_orm::DatabaseConnection, slug: &str) -> Result<Uuid, Response> {
    match projects::find_by_slug(db, slug).await {
        Ok(Some(project)) => Ok(project.id),
        Ok(None) => Err(not_found()),
        Err(e) => Err(db_error(e)),
    }
}

/// Reject anything without valid operator credentials.
pub async fn require_operator<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    request: Request,
    next: Next,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match verify_admin_basic_auth(state.app.config.admin.as_ref(), request.headers()) {
        Ok(()) => next.run(request).await,
        Err(rejection) => rejection.into_response(),
    }
}

fn db_error(e: sea_orm::DbErr) -> Response {
    // `tracing::error!` here is intentional and safe: the capture layer ignores
    // this module's target, so a failing console query cannot feed itself.
    tracing::error!(target: erno::error_reporting::COLLECTOR_TARGET, "operator query failed: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal_error" })),
    )
        .into_response()
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response()
}

impl From<IssueQuery> for IssueFilters {
    fn from(query: IssueQuery) -> Self {
        Self {
            // Set by the caller: the nested route takes it from the path, the
            // all-projects route from an optional `?project=` slug.
            project_id: None,
            status: query.status,
            source: query.source,
            q: query.q,
            release: query.release,
            hours: query.hours,
            page: query.page,
            per_page: query.per_page,
        }
    }
}

async fn respond_issues<ExtraConfig>(
    state: &CollectorState<ExtraConfig>,
    project_id: Option<Uuid>,
    query: IssueQuery,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let mut filters = IssueFilters::from(query);
    filters.project_id = project_id;
    match service::list_issues(&state.app.db, &filters).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/issues` — every project, newest activity first.
///
/// Stays un-nested: it is the console's cross-application view, and the
/// optional `?project=` slug narrows it without a second route.
pub async fn list_issues<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Query(query): Query<IssueQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match query
        .project
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(slug) => match project_id_for(&state.app.db, slug).await {
            Ok(id) => Some(id),
            Err(response) => return response,
        },
        None => None,
    };
    respond_issues(&state, project_id, query).await
}

/// `GET /api/collector/projects/{slug}/issues`
pub async fn list_project_issues<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Query(query): Query<IssueQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    respond_issues(&state, Some(project_id), query).await
}

/// `GET /api/collector/issues/counts` — every project.
///
/// Also the console nginx auth probe (`/__monitoring_auth`), which hard-codes
/// this path with no slug. It must stay here.
pub async fn issue_counts<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Query(query): Query<SeriesQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match service::issue_counts(&state.app.db, None, query.hours).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}/issues/counts`
pub async fn project_issue_counts<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Query(query): Query<SeriesQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match service::issue_counts(&state.app.db, Some(project_id), query.hours).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}/issues/{id}`
pub async fn get_issue<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match service::get_issue(&state.app.db, project_id, id).await {
        Ok(Some(body)) => Json(body).into_response(),
        Ok(None) => not_found(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}/issues/{id}/events`
pub async fn list_events<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
    Query(query): Query<EventQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match service::list_events(&state.app.db, project_id, id, query.page, query.per_page).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}/issues/{id}/series`
pub async fn issue_series<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
    Query(query): Query<SeriesQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match service::series(&state.app.db, Some(project_id), Some(id), query.hours, None).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}/series`
pub async fn global_series<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Query(query): Query<SeriesQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match service::series(
        &state.app.db,
        Some(project_id),
        None,
        query.hours,
        query.source.as_deref(),
    )
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

async fn update_status<ExtraConfig>(
    state: &CollectorState<ExtraConfig>,
    slug: &str,
    id: Uuid,
    status: IssueStatus,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match service::set_status(&state.app.db, project_id, id, status).await {
        Ok(Some(body)) => {
            // Triage decisions belong in the operator audit log.
            erno::admin_events::emit_ok(
                &state.app.db,
                &format!("error_issue.{status}"),
                None,
                json!({ "issue_id": id, "fingerprint": body.fingerprint }),
            )
            .await;
            Json(body).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/issues/{id}/resolve`
pub async fn resolve<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    update_status(&state, &slug, id, IssueStatus::Resolved).await
}

/// `POST /api/collector/projects/{slug}/issues/{id}/ignore`
pub async fn ignore<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    update_status(&state, &slug, id, IssueStatus::Ignored).await
}

/// `POST /api/collector/projects/{slug}/issues/{id}/unresolve`
pub async fn unresolve<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    update_status(&state, &slug, id, IssueStatus::Unresolved).await
}

/// `DELETE /api/collector/projects/{slug}/issues/{id}`
pub async fn delete_issue<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match service::delete_issue(&state.app.db, project_id, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(e) => db_error(e),
    }
}

/// `DELETE /api/collector/users/{id}/events`
///
/// Called by an application's account-deletion path. Uses the trusted
/// server-to-server ingest token rather than operator credentials, since it is
/// machine-to-machine.
pub async fn anonymize_user<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(id): Path<Uuid>,
    headers: axum::http::HeaderMap,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let Some(identity) =
        super::auth::authenticate(&state.app.db, &state.token_cache, &headers, None).await
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_ingest_key" })),
        )
            .into_response();
    };
    if !identity.origin.trusted {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_ingest_key" })),
        )
            .into_response();
    }

    match service::anonymize_user(&state.app.db, identity.project_id, id).await {
        Ok(rows) => Json(json!({ "anonymized": rows })).into_response(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}/releases`
pub async fn list_releases<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Query(query): Query<ReleaseQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match releases::list(&state.app.db, project_id, &query).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/releases`
///
/// Machine-to-machine: a CI pipeline posts this on deploy, so it authenticates
/// with the trusted ingest token rather than operator credentials.
pub async fn record_release<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    headers: axum::http::HeaderMap,
    Json(input): Json<RecordRelease>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let Some(identity) =
        super::auth::authenticate(&state.app.db, &state.token_cache, &headers, None).await
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_ingest_key" })),
        )
            .into_response();
    };
    if !identity.origin.trusted {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_ingest_key" })),
        )
            .into_response();
    }

    if input.version.trim().is_empty() || input.environment.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "version and environment are required" })),
        )
            .into_response();
    }

    match releases::record(&state.app.db, identity.project_id, input).await {
        Ok(model) => (
            StatusCode::CREATED,
            Json(json!({ "id": model.id, "version": model.version })),
        )
            .into_response(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}/health`
pub async fn get_health<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::health::list(&state.app.db, project_id, &state.config.health).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/health`
///
/// The application's heartbeat. Machine-to-machine, so it uses the trusted
/// ingest token rather than operator credentials.
pub async fn record_health<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    headers: axum::http::HeaderMap,
    Json(snapshot): Json<erno::health::HealthSnapshot>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let Some(identity) =
        super::auth::authenticate(&state.app.db, &state.token_cache, &headers, None).await
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_ingest_key" })),
        )
            .into_response();
    };
    if !identity.origin.trusted {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_ingest_key" })),
        )
            .into_response();
    }

    match super::health::record(&state.app.db, identity.project_id, &snapshot).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(e) => db_error(e),
    }
}

/// Window used by the uptime list when none is given.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UptimeQuery {
    #[serde(default)]
    pub hours: Option<i64>,
}

/// `GET /api/collector/projects/{slug}/uptime`
pub async fn list_checks<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Query(query): Query<UptimeQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::uptime::service::list(&state.app.db, project_id, query.hours.unwrap_or(24)).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/uptime`
pub async fn create_check<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Json(input): Json<super::uptime::service::UpsertCheck>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::uptime::service::create(&state.app.db, project_id, input).await {
        Ok(model) => (StatusCode::CREATED, Json(json!({ "id": model.id }))).into_response(),
        Err(super::uptime::service::CheckError::Invalid(message)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": message })),
        )
            .into_response(),
        Err(super::uptime::service::CheckError::Db(e)) => db_error(e),
    }
}

/// `DELETE /api/collector/projects/{slug}/uptime/{id}`
pub async fn delete_check<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::uptime::service::delete(&state.app.db, project_id, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/uptime/{id}/enable` and `/disable`
async fn set_check_enabled<ExtraConfig>(
    state: &CollectorState<ExtraConfig>,
    slug: &str,
    id: Uuid,
    enabled: bool,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::uptime::service::set_enabled(&state.app.db, project_id, id, enabled).await {
        Ok(Some(model)) => {
            Json(json!({ "id": model.id, "enabled": model.enabled })).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/uptime/{id}/enable`
pub async fn enable_check<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    set_check_enabled(&state, &slug, id, true).await
}

/// `POST /api/collector/projects/{slug}/uptime/{id}/disable`
pub async fn disable_check<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    set_check_enabled(&state, &slug, id, false).await
}

/// `GET /api/collector/projects/{slug}/status/components`
pub async fn list_components<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::status::service::list_components(&state.app.db, project_id).await {
        Ok(components) => Json(json!({ "components": components })).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/status/components`
pub async fn create_component<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Json(input): Json<super::status::service::UpsertComponent>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    if input.name.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "name is required" })),
        )
            .into_response();
    }
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::status::service::create_component(&state.app.db, project_id, input).await {
        Ok(model) => (StatusCode::CREATED, Json(json!({ "id": model.id }))).into_response(),
        Err(e) => db_error(e),
    }
}

/// `DELETE /api/collector/projects/{slug}/status/components/{id}`
pub async fn delete_component<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::status::service::delete_component(&state.app.db, project_id, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(e) => db_error(e),
    }
}

/// Body of a manual component state change.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ComponentStateBody {
    pub state: String,
}

/// `POST /api/collector/projects/{slug}/status/components/{id}/state`
pub async fn set_component_state<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
    Json(body): Json<ComponentStateBody>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::status::service::set_component_state(&state.app.db, project_id, id, &body.state)
        .await
    {
        Ok(Some(model)) => {
            Json(json!({ "id": model.id, "state": model.manual_state })).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/status/incidents`
pub async fn open_incident<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Json(input): Json<super::status::service::OpenIncident>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    if input.title.trim().is_empty() || input.body.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "title and body are required" })),
        )
            .into_response();
    }
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::status::service::open_incident(&state.app.db, project_id, input).await {
        Ok(model) => (StatusCode::CREATED, Json(json!({ "id": model.id }))).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/status/incidents/{id}/updates`
pub async fn add_incident_update<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
    Json(input): Json<super::status::service::AddUpdate>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    if input.body.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "body is required" })),
        )
            .into_response();
    }
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::status::service::add_update(&state.app.db, project_id, id, input).await {
        Ok(Some(model)) => Json(json!({ "id": model.id, "status": model.status })).into_response(),
        Ok(None) => not_found(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}/status.json`
///
/// Unauthenticated preview of the published document, for local development.
/// Relying on it in production defeats the point: a status page served by the
/// collector goes down with the collector.
///
/// Per project rather than global: one document covering every application
/// would tell each product's users about the others' outages.
pub async fn status_snapshot<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project = match projects::find_by_slug(&state.app.db, &slug).await {
        Ok(Some(project)) => project,
        Ok(None) => return not_found(),
        Err(e) => return db_error(e),
    };
    let config = &state.config.status;
    match super::status::service::build_snapshot(
        &state.app.db,
        &project,
        &config.name,
        config.refresh_seconds,
    )
    .await
    {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}/alerts`
pub async fn list_rules<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::alerting::service::list(&state.app.db, project_id).await {
        Ok(rules) => Json(json!({ "rules": rules })).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/alerts`
pub async fn create_rule<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Json(input): Json<super::alerting::service::CreateRule>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::alerting::service::create(&state.app.db, project_id, input).await {
        Ok(model) => (StatusCode::CREATED, Json(json!({ "id": model.id }))).into_response(),
        Err(super::alerting::service::RuleError::Invalid(message)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": message })),
        )
            .into_response(),
        Err(super::alerting::service::RuleError::Db(e)) => db_error(e),
    }
}

/// `DELETE /api/collector/projects/{slug}/alerts/{id}`
pub async fn delete_rule<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::alerting::service::delete(&state.app.db, project_id, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/alerts/{id}/enable` and `/disable`
async fn set_rule_enabled<ExtraConfig>(
    state: &CollectorState<ExtraConfig>,
    slug: &str,
    id: Uuid,
    enabled: bool,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::alerting::service::set_enabled(&state.app.db, project_id, id, enabled).await {
        Ok(Some(model)) => {
            Json(json!({ "id": model.id, "enabled": model.enabled })).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/alerts/{id}/enable`
pub async fn enable_rule<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    set_rule_enabled(&state, &slug, id, true).await
}

/// `POST /api/collector/projects/{slug}/alerts/{id}/disable`
pub async fn disable_rule<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    set_rule_enabled(&state, &slug, id, false).await
}

/// Body of a silence request.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SilenceBody {
    /// Minutes to stay quiet. Zero or less clears the silence.
    pub minutes: i64,
}

/// `POST /api/collector/projects/{slug}/alerts/{id}/silence`
pub async fn silence_rule<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path((slug, id)): Path<(String, Uuid)>,
    Json(body): Json<SilenceBody>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let project_id = match project_id_for(&state.app.db, &slug).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    match super::alerting::service::silence(&state.app.db, project_id, id, body.minutes).await {
        Ok(Some(model)) => Json(json!({
            "id": model.id,
            "silence_until": model.silence_until
        }))
        .into_response(),
        Ok(None) => not_found(),
        Err(e) => db_error(e),
    }
}
