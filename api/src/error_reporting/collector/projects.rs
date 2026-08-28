//! Project rows: create, list, rotate ingest tokens, boot seed.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! GET never echoes hashes or scrape bearers. Plaintext ingest tokens appear
//! only in create/rotate responses and, when generated on first boot, once on
//! stdout (never `tracing::info`).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr,
    EntityTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::api::unique_constraint::is_unique_violation;
use crate::error_reporting::config::{CollectorConfig, ErrorReportingConfig};
use crate::token::{generate_secure_token, hash_token};

use super::auth::TokenCache;
use super::cors::{origins_from_json, refresh_origins};
use super::models::project;
use super::state::CollectorState;

const SERVER_PREFIX: &str = "erns_";
const BROWSER_PREFIX: &str = "ernb_";
const SEED_SLUG: &str = "monitoring";

/// Public project view. Secrets are never included.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDto {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub cors_origins: Vec<String>,
    pub scrape_target: String,
    pub scrape_scheme: String,
    pub scrape_metrics_token_set: bool,
    pub event_retention_days: Option<i64>,
    pub issue_retention_days: Option<i64>,
    pub max_events_per_issue: Option<i64>,
    pub status_enabled: bool,
    pub status_name: String,
    pub created_at: NaiveDateTime,
}

/// Create-project body. Tokens are minted server-side.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProject {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub cors_origins: Vec<String>,
    #[serde(default)]
    pub scrape_target: String,
    #[serde(default)]
    pub scrape_scheme: Option<String>,
    #[serde(default)]
    pub scrape_metrics_token: String,
    #[serde(default)]
    pub event_retention_days: Option<i64>,
    #[serde(default)]
    pub issue_retention_days: Option<i64>,
    #[serde(default)]
    pub max_events_per_issue: Option<i64>,
    #[serde(default)]
    pub status_enabled: bool,
    #[serde(default)]
    pub status_name: String,
}

/// Patch-project body. Every field is optional; absent means "leave alone".
///
/// `slug` is deliberately absent. It is the Tempo/Loki `X-Scope-OrgID` and the
/// directory name of the published status document, so renaming one would
/// orphan a tenant and a URL. A rename is a new project.
#[derive(Debug, Clone, Deserialize)]
pub struct PatchProject {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub cors_origins: Option<Vec<String>>,
    #[serde(default)]
    pub scrape_target: Option<String>,
    #[serde(default)]
    pub scrape_scheme: Option<String>,
    /// Write-only. `GET` never echoes it.
    #[serde(default)]
    pub scrape_metrics_token: Option<String>,
    #[serde(default)]
    pub event_retention_days: Option<Option<i64>>,
    #[serde(default)]
    pub issue_retention_days: Option<Option<i64>>,
    #[serde(default)]
    pub max_events_per_issue: Option<Option<i64>>,
    #[serde(default)]
    pub status_enabled: Option<bool>,
    #[serde(default)]
    pub status_name: Option<String>,
    /// Present only so an attempt to rename can be refused loudly rather than
    /// silently ignored.
    #[serde(default)]
    pub slug: Option<String>,
}

/// Create response: the DTO plus plaintext tokens shown once.
#[derive(Debug, Clone, Serialize)]
pub struct CreateProjectResponse {
    #[serde(flatten)]
    pub project: ProjectDto,
    pub server_token: String,
    pub browser_token: String,
}

/// Rotate response: plaintext shown once.
#[derive(Debug, Clone, Serialize)]
pub struct RotateTokenResponse {
    pub token: String,
}

/// Set scrape bearer. Never echoed on GET.
#[derive(Debug, Clone, Deserialize)]
pub struct SetScrapeToken {
    pub token: String,
}

/// Why a project could not be saved.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// Submitted values are unusable.
    #[error("{0}")]
    Invalid(String),
    /// Slug already exists.
    #[error("slug is already taken")]
    DuplicateSlug,
    /// Database rejected the write.
    #[error(transparent)]
    Db(#[from] DbErr),
}

impl From<project::Model> for ProjectDto {
    fn from(model: project::Model) -> Self {
        let scrape_metrics_token_set = !model.scrape_metrics_token.is_empty();
        Self {
            id: model.id,
            slug: model.slug,
            name: model.name,
            cors_origins: origins_from_json(&model.cors_origins),
            scrape_target: model.scrape_target,
            scrape_scheme: model.scrape_scheme,
            scrape_metrics_token_set,
            event_retention_days: model.event_retention_days,
            issue_retention_days: model.issue_retention_days,
            max_events_per_issue: model.max_events_per_issue,
            status_enabled: model.status_enabled,
            status_name: model.status_name,
            created_at: model.created_at,
        }
    }
}

/// Lowercase letter, then lowercase alphanumeric / hyphen / underscore.
#[must_use]
pub fn validate_slug(slug: &str) -> bool {
    let mut chars = slug.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn mint_token(prefix: &str) -> (String, String) {
    let raw = format!("{prefix}{}", generate_secure_token(32));
    let hash = hash_token(&raw);
    (raw, hash)
}

/// Insert a project, returning plaintext tokens once.
pub async fn create(
    db: &DatabaseConnection,
    input: CreateProject,
) -> Result<(project::Model, String, String), ProjectError> {
    let slug = input.slug.trim();
    if !validate_slug(slug) {
        return Err(ProjectError::Invalid(
            "slug must start with a lowercase letter and contain only lowercase letters, digits, hyphens, or underscores".to_string(),
        ));
    }
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ProjectError::Invalid("name is required".to_string()));
    }

    let (server_token, server_hash) = mint_token(SERVER_PREFIX);
    let (browser_token, browser_hash) = mint_token(BROWSER_PREFIX);
    let cors = serde_json::to_value(
        input
            .cors_origins
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| json!([]));

    let scheme = input
        .scrape_scheme
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("https");

    let model = project::ActiveModel {
        id: Set(Uuid::new_v4()),
        slug: Set(slug.to_string()),
        name: Set(truncate(name, 200)),
        server_token_hash: Set(server_hash),
        browser_token_hash: Set(browser_hash),
        cors_origins: Set(cors),
        scrape_target: Set(input.scrape_target.trim().to_string()),
        scrape_scheme: Set(scheme.to_string()),
        scrape_metrics_token: Set(input.scrape_metrics_token),
        event_retention_days: Set(input.event_retention_days),
        issue_retention_days: Set(input.issue_retention_days),
        max_events_per_issue: Set(input.max_events_per_issue),
        status_enabled: Set(input.status_enabled),
        status_name: Set(input.status_name.trim().to_string()),
        created_at: Set(chrono::Utc::now().naive_utc()),
    };

    match model.insert(db).await {
        Ok(row) => Ok((row, server_token, browser_token)),
        Err(e) if is_unique_violation(&e) => Err(ProjectError::DuplicateSlug),
        Err(e) => Err(ProjectError::Db(e)),
    }
}

/// Every project, newest last so the seeded row is first on a fresh collector.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<ProjectDto>, DbErr> {
    let rows = project::Entity::find()
        .order_by_asc(project::Column::CreatedAt)
        .all(db)
        .await?;
    Ok(rows.into_iter().map(ProjectDto::from).collect())
}

/// One project by slug.
pub async fn find_by_slug(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<Option<project::Model>, DbErr> {
    project::Entity::find()
        .filter(project::Column::Slug.eq(slug))
        .one(db)
        .await
}

/// Update the mutable fields of a project.
///
/// # Errors
///
/// [`ProjectError::Invalid`] for a rename attempt or an empty name, and the
/// database error otherwise.
pub async fn patch(
    db: &DatabaseConnection,
    slug: &str,
    input: PatchProject,
) -> Result<Option<project::Model>, ProjectError> {
    if let Some(requested) = input.slug.as_deref().map(str::trim) {
        if requested != slug {
            return Err(ProjectError::Invalid(
                "slug is immutable: it identifies the Tempo and Loki tenant and the published status document".to_string(),
            ));
        }
    }

    let Some(row) = find_by_slug(db, slug).await? else {
        return Ok(None);
    };

    let mut active: project::ActiveModel = row.into();

    if let Some(name) = input.name.as_deref().map(str::trim) {
        if name.is_empty() {
            return Err(ProjectError::Invalid("name is required".to_string()));
        }
        active.name = Set(truncate(name, 200));
    }
    if let Some(origins) = input.cors_origins {
        let cleaned = serde_json::to_value(
            origins
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| json!([]));
        active.cors_origins = Set(cleaned);
    }
    if let Some(target) = input.scrape_target.as_deref().map(str::trim) {
        active.scrape_target = Set(target.to_string());
    }
    if let Some(scheme) = input
        .scrape_scheme
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        active.scrape_scheme = Set(scheme.to_string());
    }
    if let Some(token) = input.scrape_metrics_token {
        active.scrape_metrics_token = Set(token);
    }
    if let Some(days) = input.event_retention_days {
        active.event_retention_days = Set(days);
    }
    if let Some(days) = input.issue_retention_days {
        active.issue_retention_days = Set(days);
    }
    if let Some(cap) = input.max_events_per_issue {
        active.max_events_per_issue = Set(cap);
    }
    if let Some(enabled) = input.status_enabled {
        active.status_enabled = Set(enabled);
    }
    if let Some(name) = input.status_name.as_deref().map(str::trim) {
        active.status_name = Set(name.to_string());
    }

    Ok(Some(active.update(db).await?))
}

/// Delete a project and, by cascade, everything recorded against it.
///
/// # Errors
///
/// Returns the database error.
pub async fn delete(db: &DatabaseConnection, slug: &str) -> Result<bool, DbErr> {
    let result = project::Entity::delete_many()
        .filter(project::Column::Slug.eq(slug))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Rotate the server ingest token. Returns plaintext once.
pub async fn rotate_server(
    db: &DatabaseConnection,
    cache: &TokenCache,
    slug: &str,
) -> Result<Option<String>, DbErr> {
    rotate_ingest(db, cache, slug, true).await
}

/// Rotate the browser ingest token. Returns plaintext once.
pub async fn rotate_browser(
    db: &DatabaseConnection,
    cache: &TokenCache,
    slug: &str,
) -> Result<Option<String>, DbErr> {
    rotate_ingest(db, cache, slug, false).await
}

async fn rotate_ingest(
    db: &DatabaseConnection,
    cache: &TokenCache,
    slug: &str,
    server: bool,
) -> Result<Option<String>, DbErr> {
    let Some(row) = find_by_slug(db, slug).await? else {
        return Ok(None);
    };
    let old_hash = if server {
        row.server_token_hash.clone()
    } else {
        row.browser_token_hash.clone()
    };
    let prefix = if server {
        SERVER_PREFIX
    } else {
        BROWSER_PREFIX
    };
    let (raw, hash) = mint_token(prefix);
    let mut active: project::ActiveModel = row.into();
    if server {
        active.server_token_hash = Set(hash);
    } else {
        active.browser_token_hash = Set(hash);
    }
    active.update(db).await?;
    cache.invalidate(&old_hash);
    Ok(Some(raw))
}

/// Replace the scrape bearer. Never returned.
pub async fn set_scrape_token(
    db: &DatabaseConnection,
    slug: &str,
    token: String,
) -> Result<bool, DbErr> {
    let Some(row) = find_by_slug(db, slug).await? else {
        return Ok(false);
    };
    let mut active: project::ActiveModel = row.into();
    active.scrape_metrics_token = Set(token);
    active.update(db).await?;
    Ok(true)
}

/// Insert the `monitoring` project when the table is empty.
///
/// Used only on insert, never on later boots or ingest. Returns whether a row
/// was created.
pub async fn seed_if_empty<C: ConnectionTrait>(
    db: &C,
    error_reporting: &ErrorReportingConfig,
    collector: &CollectorConfig,
) -> Result<bool, DbErr> {
    if project::Entity::find().one(db).await?.is_some() {
        return Ok(false);
    }

    let (server_raw, generated_server) = if !collector.seed.server_token.trim().is_empty() {
        (collector.seed.server_token.trim().to_string(), false)
    } else if !error_reporting.ingest_token.trim().is_empty() {
        (error_reporting.ingest_token.trim().to_string(), false)
    } else {
        (mint_token(SERVER_PREFIX).0, true)
    };

    let (browser_raw, generated_browser) = if !collector.seed.browser_token.trim().is_empty() {
        (collector.seed.browser_token.trim().to_string(), false)
    } else {
        (mint_token(BROWSER_PREFIX).0, true)
    };

    let model = project::ActiveModel {
        id: Set(Uuid::new_v4()),
        slug: Set(SEED_SLUG.to_string()),
        name: Set("Monitoring".to_string()),
        server_token_hash: Set(hash_token(&server_raw)),
        browser_token_hash: Set(hash_token(&browser_raw)),
        cors_origins: Set(json!([])),
        scrape_target: Set(String::new()),
        scrape_scheme: Set("https".to_string()),
        scrape_metrics_token: Set(String::new()),
        event_retention_days: Set(None),
        issue_retention_days: Set(None),
        max_events_per_issue: Set(None),
        status_enabled: Set(false),
        status_name: Set(String::new()),
        created_at: Set(chrono::Utc::now().naive_utc()),
    };
    model.insert(db).await?;

    // stdout, never tracing::info — that lands in Loki.
    if generated_server {
        println!("collector: monitoring project server ingest token (shown once): {server_raw}");
    }
    if generated_browser {
        println!("collector: monitoring project browser ingest token (shown once): {browser_raw}");
    }

    Ok(true)
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}

fn project_error(err: ProjectError) -> Response {
    match err {
        ProjectError::Invalid(message) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": message })),
        )
            .into_response(),
        ProjectError::DuplicateSlug => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "slug is already taken" })),
        )
            .into_response(),
        ProjectError::Db(e) => {
            tracing::error!(target: "erno::error_reporting::collector", "project write failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "internal_error" })),
            )
                .into_response()
        }
    }
}

fn db_error(e: DbErr) -> Response {
    tracing::error!(target: "erno::error_reporting::collector", "project query failed: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal_error" })),
    )
        .into_response()
}

/// `GET /api/collector/projects`
pub async fn list_projects<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match list(&state.app.db).await {
        Ok(projects) => Json(json!({ "projects": projects })).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects`
pub async fn create_project<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Json(input): Json<CreateProject>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match create(&state.app.db, input).await {
        Ok((row, server_token, browser_token)) => {
            // Union first so a failed reload still allows this project's origins
            // on the replica that handled the create.
            state
                .origin_set
                .extend(origins_from_json(&row.cors_origins));
            if let Err(e) = refresh_origins(
                &state.origin_set,
                &state.app.db,
                &state.app.config.cors.allowed_origins,
            )
            .await
            {
                tracing::error!(
                    target: "erno::error_reporting::collector",
                    "could not reload CORS origins after create: {e}"
                );
            }
            (
                StatusCode::CREATED,
                Json(CreateProjectResponse {
                    project: ProjectDto::from(row),
                    server_token,
                    browser_token,
                }),
            )
                .into_response()
        }
        Err(e) => project_error(e),
    }
}

/// `PATCH /api/collector/projects/{slug}`
pub async fn patch_project<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Json(input): Json<PatchProject>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match patch(&state.app.db, &slug, input).await {
        Ok(Some(row)) => {
            // Origins may have changed; reload before answering so the next
            // preflight on this replica already knows.
            state
                .origin_set
                .extend(origins_from_json(&row.cors_origins));
            if let Err(e) = refresh_origins(
                &state.origin_set,
                &state.app.db,
                &state.app.config.cors.allowed_origins,
            )
            .await
            {
                tracing::error!(
                    target: "erno::error_reporting::collector",
                    "could not reload CORS origins after patch: {e}"
                );
            }
            Json(ProjectDto::from(row)).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response(),
        Err(e) => project_error(e),
    }
}

/// Query string of the delete route.
#[derive(Debug, Clone, Deserialize)]
pub struct DeleteProjectQuery {
    #[serde(default)]
    pub force: Option<String>,
}

/// `DELETE /api/collector/projects/{slug}?force=1`
///
/// Deleting a project cascades to every issue, event, release, health row,
/// uptime check, status component and alert rule recorded against it. `force`
/// is required so that is never one click away in the console.
///
/// Tempo and Loki tenants are not reaped: their data is keyed by slug in a
/// store the collector does not own.
pub async fn delete_project<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Query(query): Query<DeleteProjectQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    if query.force.as_deref() != Some("1") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "deleting a project removes every issue, event, release, uptime check and alert rule recorded against it; repeat with ?force=1"
            })),
        )
            .into_response();
    }

    match delete(&state.app.db, &slug).await {
        Ok(true) => {
            if let Err(e) = refresh_origins(
                &state.origin_set,
                &state.app.db,
                &state.app.config.cors.allowed_origins,
            )
            .await
            {
                tracing::error!(
                    target: "erno::error_reporting::collector",
                    "could not reload CORS origins after delete: {e}"
                );
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response(),
        Err(e) => db_error(e),
    }
}

/// `GET /api/collector/projects/{slug}`
pub async fn get_project<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match find_by_slug(&state.app.db, &slug).await {
        Ok(Some(row)) => Json(ProjectDto::from(row)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/tokens/server`
pub async fn rotate_server_token<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match rotate_server(&state.app.db, &state.token_cache, &slug).await {
        Ok(Some(token)) => Json(RotateTokenResponse { token }).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/tokens/browser`
pub async fn rotate_browser_token<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match rotate_browser(&state.app.db, &state.token_cache, &slug).await {
        Ok(Some(token)) => Json(RotateTokenResponse { token }).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response(),
        Err(e) => db_error(e),
    }
}

/// `POST /api/collector/projects/{slug}/tokens/scrape`
pub async fn set_scrape_metrics_token<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    Path(slug): Path<String>,
    Json(body): Json<SetScrapeToken>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match set_scrape_token(&state.app.db, &slug, body.token).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response(),
        Err(e) => db_error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_rules_match_erno_new() {
        assert!(validate_slug("teryon"));
        assert!(validate_slug("a"));
        assert!(validate_slug("acme-app_1"));
        assert!(!validate_slug(""));
        assert!(!validate_slug("Teryon"));
        assert!(!validate_slug("1teryon"));
        assert!(!validate_slug("-x"));
        assert!(!validate_slug("has space"));
    }
}
