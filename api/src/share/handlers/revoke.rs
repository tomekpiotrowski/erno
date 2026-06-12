use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::{
    app::App,
    auth::current_user::CurrentUser,
    share::models::{share, share_grant},
};

/// `DELETE /shares/{id}` — revoke a whole share (link token and all grants).
///
/// Sets `revoked_at` and fans the revocation out to every live connection
/// holding the share, which receives a `share-revoked` broadcast and stops
/// receiving push events for it immediately.
pub async fn revoke<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
    Path(share_id): Path<Uuid>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let share = match share::Entity::find_by_id(share_id).one(&app.db).await {
        Ok(Some(share)) => share,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if share.owner_id != current_user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mut active = share.into_active_model();
    active.revoked_at = Set(Some(Utc::now().naive_utc()));
    if let Err(e) = active.update(&app.db).await {
        tracing::error!("Failed to revoke share {}: {:?}", share_id, e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    app.websocket_connections
        .remove_share_everywhere(share_id)
        .await;

    StatusCode::NO_CONTENT.into_response()
}

/// `DELETE /shares/{id}/grants/{user_id}` — revoke a single grant.
///
/// The link token and other grants stay live; only the named user's
/// connections lose the share.
pub async fn revoke_grant<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
    Path((share_id, user_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let share = match share::Entity::find_by_id(share_id).one(&app.db).await {
        Ok(Some(share)) => share,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if share.owner_id != current_user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let grant = match share_grant::Entity::find()
        .filter(share_grant::Column::ShareId.eq(share_id))
        .filter(share_grant::Column::UserId.eq(user_id))
        .filter(share_grant::Column::RevokedAt.is_null())
        .one(&app.db)
        .await
    {
        Ok(Some(grant)) => grant,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let mut active = grant.into_active_model();
    active.revoked_at = Set(Some(Utc::now().naive_utc()));
    if let Err(e) = active.update(&app.db).await {
        tracing::error!("Failed to revoke grant on share {}: {:?}", share_id, e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    app.websocket_connections
        .remove_share_from_user(share_id, user_id)
        .await;

    StatusCode::NO_CONTENT.into_response()
}
