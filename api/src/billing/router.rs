use axum::{
    routing::post,
    Router,
};

use crate::{
    app::App,
    billing::handlers::{admin_gift::admin_gift, checkout::checkout, portal::portal, webhooks::webhooks},
};

/// Mount all billing routes.
///
/// Usage in your app router:
/// ```rust,ignore
/// app_router.nest("/billing", billing_router(app.clone()));
/// ```
///
/// Routes:
/// - `POST /checkout`      — create Stripe Checkout Session (requires JWT)
/// - `POST /portal`        — create Stripe Customer Portal session (requires JWT)
/// - `POST /webhooks`      — receive Stripe webhook events (no auth, HMAC-validated)
/// - `POST /admin/gift`    — gift a subscription to a user (admin bearer token)
pub fn billing_router<ExtraConfig: Clone + Send + Sync + 'static>(
    app: App<ExtraConfig>,
) -> Router {
    Router::new()
        .route("/checkout", post(checkout::<ExtraConfig>))
        .route("/portal", post(portal::<ExtraConfig>))
        .route("/webhooks", post(webhooks::<ExtraConfig>))
        .route("/admin/gift", post(admin_gift::<ExtraConfig>))
        .with_state(app)
}
