use axum::{routing::get, Router};
use tower_http::trace::TraceLayer;

use crate::{
    api, app::App, rate_limiting::middleware::rate_limit_middleware,
    websocket::auth::authenticated_ws_handler,
};

pub fn router(app: App, app_router: fn(App) -> Router) -> Router {
    let rate_limit_state = app.rate_limit_state.clone();
    let rate_limiting_enabled = app.config.rate_limiting.enabled;

    let mut api_router = Router::new().nest("/api", app_router(app.clone()));

    // Apply rate limiting middleware if enabled
    if rate_limiting_enabled {
        api_router = api_router.layer(axum::middleware::from_fn_with_state(
            rate_limit_state,
            rate_limit_middleware,
        ));
    }

    Router::new()
        .route("/liveness", get(api::health_checks::ok))
        .route("/readiness", get(api::health_checks::ok))
        .route("/ws", get(authenticated_ws_handler))
        .with_state(app)
        .merge(api_router)
        .layer(TraceLayer::new_for_http())
}
