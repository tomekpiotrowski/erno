use axum::{
    extract::{ws::WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use tower_http::trace::TraceLayer;

use crate::{api, app::App, rate_limiting::middleware::rate_limit_middleware, websocket::connections::Connections};

pub fn router(app: App, app_router: fn(App) -> Router) -> Router {
    let rate_limit_state = app.rate_limit_state.clone();
    let rate_limiting_enabled = app.config.rate_limiting.enabled;

    let mut api_router = Router::new()
        .nest("/api", app_router(app));

    // Apply rate limiting middleware if enabled
    if rate_limiting_enabled {
        api_router = api_router.layer(axum::middleware::from_fn_with_state(rate_limit_state, rate_limit_middleware));
    }

    Router::new()
        .route("/liveness", get(api::health_checks::ok))
        .route("/readiness", get(api::health_checks::ok))
        .route("/ws", get(websocket_handler))
        .merge(api_router)
        .layer(TraceLayer::new_for_http())
}

async fn websocket_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(socket: WebSocket) {
    let connection_manager = Connections::new();
    connection_manager.handle_socket(socket).await;
}
