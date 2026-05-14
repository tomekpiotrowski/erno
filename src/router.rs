use axum::{extract::Request, middleware::Next, response::Response, routing::get, Router};
use tower_http::trace::TraceLayer;

use crate::{
    api, app::App,
    rate_limiting::middleware::{rate_limit_middleware, RateLimitActionExt},
    rate_limiting::action::RateLimitAction,
    websocket::auth::authenticated_ws_handler,
};

/// Tags each request with a rate-limit action name based on path so that the
/// rate-limit middleware can apply per-endpoint quotas.  This runs as the
/// outermost layer (before rate limiting) so the extension is available when
/// `rate_limit_middleware` inspects it.
async fn tag_rate_limit_action(mut req: Request, next: Next) -> Response {
    let action = match req.uri().path() {
        "/api/auth/login" => "user_login",
        "/api/auth/register" => "user_create",
        "/api/auth/email/verify" => "user_verify",
        "/api/auth/email/resend-verification" => "resend_verification",
        "/api/auth/password-reset/request" => "password_reset_request",
        "/api/auth/password-reset/confirm" => "password_reset_confirm",
        _ => "default",
    };
    req.extensions_mut()
        .insert(RateLimitActionExt(RateLimitAction::new(action)));
    next.run(req).await
}

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

    // Apply rate limiting to all API and WebSocket routes.
    // tag_rate_limit_action is applied last so it runs first (outermost layer),
    // ensuring the action extension is set before rate_limit_middleware reads it.
    if rate_limiting_enabled {
        rate_limited = rate_limited
            .layer(axum::middleware::from_fn_with_state(
                rate_limit_state,
                rate_limit_middleware,
            ))
            .layer(axum::middleware::from_fn(tag_rate_limit_action));
    }

    // Health check endpoints are excluded from rate limiting intentionally
    Router::new()
        .route("/liveness", get(api::health_checks::ok))
        .route("/readiness", get(api::health_checks::ok))
        .merge(rate_limited)
        .layer(TraceLayer::new_for_http())
}
