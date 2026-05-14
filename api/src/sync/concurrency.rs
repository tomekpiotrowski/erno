use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::sync::syncable::Syncable;

/// Returned by `check_sync_version` when the client's `sync_seq` is stale.
///
/// Implements `IntoResponse` — use `?` or `.into_response()` in a handler to
/// emit a 409 Conflict with the server's current record so the client can
/// present conflict resolution UI without an extra round-trip.
#[derive(Debug)]
pub struct SyncConflict<M: Serialize> {
    pub server_sync_seq: i64,
    pub server_record: M,
}

impl<M: Serialize> IntoResponse for SyncConflict<M> {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": "conflict",
            "server_sync_seq": self.server_sync_seq,
            "server_record": serde_json::to_value(&self.server_record)
                .unwrap_or(serde_json::Value::Null),
        });
        (StatusCode::CONFLICT, Json(body)).into_response()
    }
}

/// Check that the client's `sync_seq` matches the server record before applying a write.
///
/// Returns `Ok(())` if the versions match, or `Err(SyncConflict)` containing
/// the current server record if they diverge.
///
/// # Example
///
/// ```rust,ignore
/// let post = post::Entity::find_by_id(id).one(&app.db).await?.ok_or(not_found())?;
/// if let Err(conflict) = check_sync_version::<post::Entity>(&post, body.sync_seq) {
///     return conflict.into_response();
/// }
/// // proceed with update
/// ```
pub fn check_sync_version<E>(
    entity: &E::Model,
    client_seq: i64,
) -> Result<(), SyncConflict<E::Model>>
where
    E: Syncable,
    E::Model: Serialize + Clone,
{
    let server_seq = E::sync_seq(entity);
    if client_seq != server_seq {
        Err(SyncConflict {
            server_sync_seq: server_seq,
            server_record: entity.clone(),
        })
    } else {
        Ok(())
    }
}
