use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use sea_orm::{ColumnTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use crate::{
    app::App, auth::current_user::CurrentUser, policy::Policy, sync::from_user::FromUser,
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
    E::Model: serde::Serialize + serde::de::DeserializeOwned,
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let policy = E::Policy::from_user(&user);

    let base_query = E::find().filter(E::sync_seq_column().gt(params.since));

    let items = match policy.readable(base_query).all(&app.db).await {
        Ok(items) => items,
        Err(e) => {
            tracing::error!("sync_delta error for {}: {:?}", E::entity_type(), e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let next_since = items
        .iter()
        .map(|m| E::sync_seq(m))
        .max()
        .unwrap_or(params.since);

    Json(SyncDeltaResponse { items, next_since }).into_response()
}
