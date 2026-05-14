use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use stripe::{
    CheckoutSession, CheckoutSessionMode, Client, CreateCheckoutSession,
    CreateCheckoutSessionLineItems, CreateCustomer, Customer,
};

use crate::{
    app::App,
    auth::current_user::CurrentUser,
    billing::models::stripe_subscription,
};

#[derive(Debug, Deserialize)]
pub struct CheckoutRequest {
    pub plan: String,
}

#[derive(Debug, Serialize)]
pub struct CheckoutResponse {
    pub url: String,
}

pub async fn checkout<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
    Json(req): Json<CheckoutRequest>,
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

    let price_id = match stripe_config.price_ids.get(&req.plan) {
        Some(id) => id.clone(),
        None => {
            return (
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "Unknown plan"})),
            )
                .into_response()
        }
    };

    let client = Client::new(&stripe_config.secret_key);

    // Reuse existing Stripe customer ID if available
    let customer_id = find_existing_customer(&app.db, current_user.id).await;

    let customer_id = match customer_id {
        Some(id) => id,
        None => {
            let mut create_customer = CreateCustomer::new();
            create_customer.email = Some(&current_user.email);
            let customer = match Customer::create(&client, create_customer).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to create Stripe customer: {}", e);
                    return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            };
            customer.id.to_string()
        }
    };

    let user_id_str = current_user.id.to_string();
    let mut params = CreateCheckoutSession::new();
    params.mode = Some(CheckoutSessionMode::Subscription);
    params.customer = customer_id.parse::<stripe::CustomerId>().ok();
    params.success_url = Some(&stripe_config.success_url);
    params.cancel_url = Some(&stripe_config.cancel_url);
    params.automatic_tax = Some(stripe::CreateCheckoutSessionAutomaticTax {
        enabled: true,
        liability: None,
    });
    params.metadata = Some(std::collections::HashMap::from([
        ("user_id".to_string(), user_id_str),
        ("plan".to_string(), req.plan.clone()),
    ]));
    params.line_items = Some(vec![CreateCheckoutSessionLineItems {
        price: Some(price_id),
        quantity: Some(1),
        ..Default::default()
    }]);

    let session = match CheckoutSession::create(&client, params).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create Stripe Checkout Session: {}", e);
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let url = match session.url {
        Some(u) => u,
        None => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    Json(CheckoutResponse { url }).into_response()
}

async fn find_existing_customer(
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
