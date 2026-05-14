use axum::{routing::get, Router};
use tower_http::trace::TraceLayer;

use crate::{
    api, app::App, rate_limiting::middleware::rate_limit_middleware,
    websocket::auth::authenticated_ws_handler,
};

pub fn router<ExtraConfig>(
    app: App<ExtraConfig>,
    app_router: fn(App<ExtraConfig>) -> Router,
) -> Router
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let rate_limit_state = app.rate_limit_state.clone();
    let rate_limiting_enabled = app.config.rate_limiting.enabled;

    // WebSocket route needs App state resolved before merging into the rate-limited group
    let ws_router = Router::new()
        .route("/ws", get(authenticated_ws_handler))
        .with_state(app.clone());

    let mut rate_limited = Router::new()
        .nest("/api", app_router(app))
        .merge(ws_router);

    // Apply rate limiting to all API and WebSocket routes
    if rate_limiting_enabled {
        rate_limited = rate_limited.layer(axum::middleware::from_fn_with_state(
            rate_limit_state,
            rate_limit_middleware,
        ));
    }

    // Health check endpoints are excluded from rate limiting intentionally
    Router::new()
        .route("/liveness", get(api::health_checks::ok))
        .route("/readiness", get(api::health_checks::ok))
        .merge(rate_limited)
        .layer(TraceLayer::new_for_http())
}
