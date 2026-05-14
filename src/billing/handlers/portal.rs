use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use stripe::{BillingPortalSession, Client, CreateBillingPortalSession};

use crate::{
    app::App,
    auth::current_user::CurrentUser,
    billing::models::stripe_subscription,
};

#[derive(Debug, Serialize)]
pub struct PortalResponse {
    pub url: String,
}

pub async fn portal<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
) -> impl IntoResponse {
    let stripe_config = match &app.config.stripe {
        Some(c) => c.clone(),
        None => {
            return (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "Stripe is not configured",
            )
                .into_response()
        }
    };

    let customer_id = match find_customer_id(&app.db, current_user.id).await {
        Some(id) => id,
        None => {
            return (
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "No Stripe subscription found"})),
            )
                .into_response()
        }
    };

    let client = Client::new(&stripe_config.secret_key);
    let Ok(cid) = customer_id.parse::<stripe::CustomerId>() else {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut params = CreateBillingPortalSession::new(cid);
    params.return_url = Some(&stripe_config.portal_return_url);

    let session = match BillingPortalSession::create(&client, params).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create Stripe Portal session: {}", e);
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(PortalResponse {
        url: session.url,
    })
    .into_response()
}

async fn find_customer_id(
    db: &sea_orm::DatabaseConnection,
    user_id: uuid::Uuid,
) -> Option<String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
    stripe_subscription::Entity::find()
        .filter(stripe_subscription::Column::UserId.eq(user_id))
        .order_by_desc(stripe_subscription::Column::CreatedAt)
        .one(db)
        .await
        .ok()
        .flatten()
        .map(|s| s.stripe_customer_id)
}
