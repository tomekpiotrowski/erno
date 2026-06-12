use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::{NaiveDateTime, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::App,
    auth::current_user::CurrentUser,
    database::models::user,
    share::handlers::grant::issue_grant,
    share::models::share::{self, SharePermission},
    token::{generate_secure_token, hash_token},
};

/// Length of raw share link tokens: 43 base-62 chars ≈ 256 bits of entropy.
pub const SHARE_TOKEN_LENGTH: usize = 43;

#[derive(Debug, Deserialize)]
pub struct CreateShareRequest {
    pub entity_type: String,
    pub entity_id: Uuid,
    /// Mint a secret link token for this share. The raw token is returned
    /// once in the response and never stored.
    #[serde(default)]
    pub link: bool,
    /// Users to grant access directly. Grants are active immediately —
    /// recipients are notified, no acceptance step.
    #[serde(default)]
    pub recipient_user_ids: Vec<Uuid>,
    pub expires_at: Option<NaiveDateTime>,
    /// Reserved: only `read` is accepted in v1.
    pub permission: Option<SharePermission>,
}

#[derive(Debug, Serialize)]
pub struct CreateShareResponse {
    pub id: Uuid,
    /// The raw link token — shown exactly once, only when `link` was requested.
    pub token: Option<String>,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub permission: SharePermission,
    pub expires_at: Option<NaiveDateTime>,
    pub granted_user_ids: Vec<Uuid>,
}

pub async fn create<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
    Json(req): Json<CreateShareRequest>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    if matches!(req.permission, Some(SharePermission::Write)) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Write shares are not supported yet",
        )
            .into_response();
    }

    if !req.link && req.recipient_user_ids.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "A share needs a link or at least one recipient",
        )
            .into_response();
    }

    if !app.sync_registry.is_shareable(&req.entity_type) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Entity type is not shareable",
        )
            .into_response();
    }

    // Sharing requires the same authority as updating the entity.
    if !app
        .sync_registry
        .can_user_share(&app.db, &req.entity_type, req.entity_id, &current_user.user)
        .await
    {
        return StatusCode::FORBIDDEN.into_response();
    }

    // Validate recipients exist up front so we fail before creating the share.
    if !req.recipient_user_ids.is_empty() {
        let found = match user::Entity::find()
            .filter(user::Column::Id.is_in(req.recipient_user_ids.clone()))
            .all(&app.db)
            .await
        {
            Ok(users) => users.len(),
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        if found != req.recipient_user_ids.len() {
            return (StatusCode::UNPROCESSABLE_ENTITY, "Unknown recipient user").into_response();
        }
    }

    let raw_token = req.link.then(|| generate_secure_token(SHARE_TOKEN_LENGTH));

    let now = Utc::now().naive_utc();
    let share = share::ActiveModel {
        id: Set(Uuid::new_v4()),
        token_hash: Set(raw_token.as_deref().map(hash_token)),
        entity_type: Set(req.entity_type.clone()),
        entity_id: Set(req.entity_id),
        owner_id: Set(current_user.id),
        permission: Set(SharePermission::Read),
        expires_at: Set(req.expires_at),
        revoked_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };

    let share = match share.insert(&app.db).await {
        Ok(share) => share,
        Err(e) => {
            tracing::error!("Failed to create share: {:?}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut granted_user_ids = Vec::with_capacity(req.recipient_user_ids.len());
    for user_id in &req.recipient_user_ids {
        match issue_grant(&app, &share, *user_id).await {
            Ok(()) => granted_user_ids.push(*user_id),
            Err(e) => {
                tracing::error!("Failed to grant share {} to {}: {:?}", share.id, user_id, e);
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    }

    (
        StatusCode::CREATED,
        Json(CreateShareResponse {
            id: share.id,
            token: raw_token,
            entity_type: share.entity_type,
            entity_id: share.entity_id,
            permission: share.permission,
            expires_at: share.expires_at,
            granted_user_ids,
        }),
    )
        .into_response()
}
