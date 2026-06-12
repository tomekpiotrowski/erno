use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DbErr, EntityTrait, IntoActiveModel, QueryFilter, Set,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    app::App,
    auth::current_user::CurrentUser,
    share::models::{share, share_grant},
    share::principal::ActiveShare,
};

/// Create (or re-activate) a grant of `share` to `user_id` and fan the share
/// into the recipient's live WebSocket connections, notifying them with a
/// `share-granted` broadcast. The grant is effective immediately.
///
/// `notified_at` is set when at least one live connection received the
/// notification.
pub(crate) async fn issue_grant<ExtraConfig>(
    app: &App<ExtraConfig>,
    share: &share::Model,
    user_id: Uuid,
) -> Result<(), DbErr> {
    let now = Utc::now().naive_utc();

    let existing = share_grant::Entity::find()
        .filter(share_grant::Column::ShareId.eq(share.id))
        .filter(share_grant::Column::UserId.eq(user_id))
        .one(&app.db)
        .await?;

    let grant_id = match existing {
        Some(grant) => {
            let id = grant.id;
            let mut active = grant.into_active_model();
            active.revoked_at = Set(None);
            active.update(&app.db).await?;
            id
        }
        None => {
            let grant = share_grant::ActiveModel {
                id: Set(Uuid::new_v4()),
                share_id: Set(share.id),
                user_id: Set(user_id),
                notified_at: Set(None),
                revoked_at: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            };
            grant.insert(&app.db).await?.id
        }
    };

    // Live fan-in: recipients with open connections start receiving push
    // events for the shared entity without reconnecting.
    let notified = app
        .websocket_connections
        .add_share_to_user(user_id, ActiveShare::from(share))
        .await;

    if notified > 0 {
        let mut active = share_grant::ActiveModel {
            id: Set(grant_id),
            ..Default::default()
        };
        active.notified_at = Set(Some(now));
        active.update(&app.db).await?;
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct GrantRequest {
    pub user_id: Uuid,
}

/// `POST /shares/{id}/grants` — add a recipient to an existing share.
pub async fn grant<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
    Path(share_id): Path<Uuid>,
    Json(req): Json<GrantRequest>,
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

    if !share.is_active(Utc::now().naive_utc()) {
        return (StatusCode::UNPROCESSABLE_ENTITY, "Share is no longer active").into_response();
    }

    let recipient = match crate::database::models::user::Entity::find_by_id(req.user_id)
        .one(&app.db)
        .await
    {
        Ok(Some(user)) => user,
        Ok(None) => {
            return (StatusCode::UNPROCESSABLE_ENTITY, "Unknown recipient user").into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    match issue_grant(&app, &share, recipient.id).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => {
            tracing::error!("Failed to grant share {}: {:?}", share.id, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
