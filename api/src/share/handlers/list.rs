use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::App,
    auth::current_user::CurrentUser,
    share::models::{share, share_grant},
};

#[derive(Debug, Deserialize)]
pub struct ListSharesQuery {
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct ShareWithGrants {
    #[serde(flatten)]
    pub share: share::Model,
    /// Whether this share has a secret link (the token itself is never returned).
    pub has_link: bool,
    pub grants: Vec<share_grant::Model>,
}

/// `GET /shares?entity_type=&entity_id=` — list the current user's shares,
/// optionally filtered to one entity, with their grants.
pub async fn list<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
    Query(query): Query<ListSharesQuery>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let mut shares_query = share::Entity::find()
        .filter(share::Column::OwnerId.eq(current_user.id))
        .order_by_asc(share::Column::CreatedAt);

    if let Some(entity_type) = &query.entity_type {
        shares_query = shares_query.filter(share::Column::EntityType.eq(entity_type));
    }
    if let Some(entity_id) = query.entity_id {
        shares_query = shares_query.filter(share::Column::EntityId.eq(entity_id));
    }

    let shares = match shares_query
        .find_with_related(share_grant::Entity)
        .all(&app.db)
        .await
    {
        Ok(shares) => shares,
        Err(e) => {
            tracing::error!("Failed to list shares: {:?}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let items: Vec<ShareWithGrants> = shares
        .into_iter()
        .map(|(share, grants)| ShareWithGrants {
            has_link: share.token_hash.is_some(),
            share,
            grants,
        })
        .collect();

    Json(items).into_response()
}
