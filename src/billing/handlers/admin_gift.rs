use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, Set};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    app::App,
    billing::{
        handlers::webhooks::update_user_subscription_cache,
        models::gift_subscription,
    },
};

#[derive(Debug, Deserialize)]
pub struct GiftRequest {
    pub user_id: Uuid,
    pub plan: String,
    pub duration_days: u32,
}

pub async fn admin_gift<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
    headers: HeaderMap,
    Json(req): Json<GiftRequest>,
) -> impl IntoResponse {
    let stripe_config = match &app.config.stripe {
        Some(c) => c.clone(),
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let provided_token = match headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    {
        Some(t) => t.to_owned(),
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    if provided_token != stripe_config.admin_token {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let active_until = Utc::now().naive_utc()
        + chrono::Duration::days(req.duration_days as i64);
    let now = Utc::now().naive_utc();

    let row = gift_subscription::ActiveModel {
        user_id: Set(req.user_id),
        plan: Set(req.plan.clone()),
        active_until: Set(active_until),
        created_at: Set(now),
        ..Default::default()
    };

    let inserted = match row.insert(&app.db).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to insert gift subscription: {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let Err(e) = update_user_subscription_cache(
        &app.db,
        req.user_id,
        Some(inserted.id),
        Some("gift".to_string()),
        Some(req.plan),
    )
    .await
    {
        tracing::error!("Failed to update user subscription cache: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::CREATED.into_response()
}
