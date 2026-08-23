use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use sea_orm::{ColumnTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use crate::{
    app::App,
    auth::current_user::CurrentUser,
    policy::Policy,
    share::principal::{FromPrincipal, Principal},
    sync::from_user::FromUser,
    sync::syncable::Syncable,
};

#[derive(Debug, Deserialize)]
pub struct SyncDeltaQuery {
    /// Return entities with `sync_seq` strictly greater than this value.
    /// Pass `0` (or omit) for a full initial sync.
    #[serde(default)]
    pub since: i64,
}

#[derive(Debug, Serialize)]
pub struct SyncDeltaResponse<T: Serialize> {
    pub items: Vec<T>,
    /// The highest `sync_seq` in this batch. Pass as `since` in the next poll.
    pub next_since: i64,
}

/// Generic delta sync handler. Mount one per syncable entity in the app router:
///
/// ```rust,ignore
/// .route("/posts/sync", get(sync_delta::<post::Entity, _>))
/// ```
///
/// Returns all records the current user can read (via `policy.readable()`) whose
/// `sync_seq` is greater than `since`. Soft-deleted records (`deleted_at IS NOT NULL`)
/// are included — clients should remove them locally when `deleted_at` is set.
pub async fn sync_delta<E, ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    CurrentUser { user, .. }: CurrentUser,
    Query(params): Query<SyncDeltaQuery>,
) -> impl IntoResponse
where
    E: Syncable,
    E::Policy: FromUser,
    E::Model: serde::Serialize + serde::de::DeserializeOwned,
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let policy = E::Policy::from_user(&user);
    run_delta::<E>(&app.db, policy, params).await
}

/// Share-aware variant of [`sync_delta`]. Mount one per shareable entity:
///
/// ```rust,ignore
/// .route("/posts/sync", get(sync_delta_shared::<post::Entity, _>))
/// ```
///
/// The policy is built from a [`Principal`] — an optional authenticated user
/// plus any active shares carried by the request (`X-Erno-Share` header tokens
/// or account grants) — so anonymous link visitors and grant recipients receive
/// the rows their shares cover in addition to (for users) their own data.
pub async fn sync_delta_shared<E, ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    principal: Principal,
    Query(params): Query<SyncDeltaQuery>,
) -> impl IntoResponse
where
    E: Syncable,
    E::Policy: FromPrincipal,
    E::Model: serde::Serialize + serde::de::DeserializeOwned,
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let policy = E::Policy::from_principal(&principal);
    run_delta::<E>(&app.db, policy, params).await
}

async fn run_delta<E>(
    db: &sea_orm::DatabaseConnection,
    policy: E::Policy,
    params: SyncDeltaQuery,
) -> axum::response::Response
where
    E: Syncable,
    E::Model: serde::Serialize + serde::de::DeserializeOwned,
{
    let base_query = E::find().filter(E::sync_seq_column().gt(params.since));

    // Labelled by entity type, which is a fixed set declared at boot — never by
    // user or by cursor, which would be unbounded.
    let timer = crate::metrics::OperationTimer::start(
        "erno_sync_delta_duration_seconds",
        "erno_sync_delta_total",
        "entity",
        E::entity_type(),
    );
    let result = policy.readable(base_query).all(db).await;
    timer.finish(&result);

    let items = match result {
        Ok(items) => items,
        Err(e) => {
            tracing::error!("sync_delta error for {}: {:?}", E::entity_type(), e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Delta size is the signal that a client is about to have a bad time: a
    // pull returning tens of thousands of rows means someone has been offline
    // for a long while, or a backfill has rewritten everything.
    metrics::histogram!("erno_sync_delta_rows",
        "entity" => E::entity_type(),
    )
    .record(items.len() as f64);

    let next_since = items
        .iter()
        .map(|m| E::sync_seq(m))
        .max()
        .unwrap_or(params.since);

    Json(SyncDeltaResponse { items, next_since }).into_response()
}
