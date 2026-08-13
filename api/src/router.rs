use axum::{
    extract::Request, http::HeaderValue, middleware::Next, response::Response, routing::get, Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::{
    admin::admin_router,
    api,
    app::App,
    auth::router::auth_router,
    config::EmailConfig,
    dev,
    environment::Environment,
    metrics::{self, http::metrics_middleware, MetricsEndpointState},
    rate_limiting::action::RateLimitAction,
    rate_limiting::middleware::{rate_limit_middleware, RateLimitActionExt},
    websocket::auth::authenticated_ws_handler,
};

/// Tags each request with a rate-limit action name based on path so that the
/// rate-limit middleware can apply per-endpoint quotas.  This runs as the
/// outermost layer (before rate limiting) so the extension is available when
/// `rate_limit_middleware` inspects it.
async fn tag_rate_limit_action(mut req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let action = if path.starts_with("/admin/api") {
        "admin"
    } else {
        match path {
            "/api/auth/login" => "user_login",
            "/api/auth/register" => "user_create",
            "/api/auth/email/verify" => "user_verify",
            "/api/auth/email/resend-verification" => "resend_verification",
            "/api/auth/password-reset/request" => "password_reset_request",
            "/api/auth/password-reset/confirm" => "password_reset_confirm",
            "/api/account" => "account_delete",
            _ => "default",
        }
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
    let metrics_enabled = app.config.metrics.enabled;
    let cors_origins: Vec<HeaderValue> = cors_origin_list(&app.config.cors.allowed_origins);
    let metrics_state = MetricsEndpointState {
        handle: app.prometheus_handle.clone(),
        auth_token: app.config.metrics.auth_token.clone(),
    };
    let metrics_path = app.config.metrics.path.clone();
    let is_dev_mock = app.environment == Environment::Development
        && matches!(&app.config.email, EmailConfig::Mock);

    // WebSocket route needs App state resolved before merging into the rate-limited group
    let ws_router = Router::new()
        .route("/ws", get(authenticated_ws_handler))
        .with_state(app.clone());

    let app_for_dev = app.clone();
    let admin = admin_router(app.clone());
    // Auth routes are auto-mounted alongside user routes under /api.
    let mut rate_limited = Router::new()
        .nest("/api", auth_router(app.clone()).merge(app_router(app)))
        .merge(ws_router);

    if let Some(admin_router) = admin {
        rate_limited = rate_limited.nest("/admin/api", admin_router);
    }

    if metrics_enabled {
        rate_limited = rate_limited.layer(axum::middleware::from_fn(metrics_middleware));
    }

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

    // Health check and metrics endpoints are excluded from rate limiting intentionally
    let mut base = Router::new()
        .route("/liveness", get(api::health_checks::ok))
        .route("/readiness", get(api::health_checks::ok))
        .merge(rate_limited)
        .layer(TraceLayer::new_for_http());

    if metrics_enabled {
        base = base.route(
            &metrics_path,
            get(metrics::metrics_handler).with_state(metrics_state),
        );
    }

    if is_dev_mock {
        base = base.merge(dev::router::dev_router(app_for_dev));
    }

    if !cors_origins.is_empty() {
        base = base.layer(
            CorsLayer::new()
                .allow_origin(cors_origins)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any)
                .allow_credentials(false),
        );
    }

    base
}

/// Configured CORS origins plus any `ERNO_DEV_CORS_ORIGINS` (comma-separated)
/// injected by `erno dev --ios` / `--android`.
pub fn cors_origin_list(configured: &[String]) -> Vec<HeaderValue> {
    configured
        .iter()
        .cloned()
        .chain(parse_extra_cors_origins(
            &std::env::var("ERNO_DEV_CORS_ORIGINS").unwrap_or_default(),
        ))
        .filter_map(|o| o.parse().ok())
        .collect()
}

pub fn parse_extra_cors_origins(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod extra_cors_tests {
    use super::parse_extra_cors_origins;

    #[test]
    fn splits_and_trims_extra_origins() {
        assert_eq!(
            parse_extra_cors_origins(" http://192.168.1.5:4200, capacitor://localhost ,"),
            vec![
                "http://192.168.1.5:4200".to_string(),
                "capacitor://localhost".to_string()
            ]
        );
        assert!(parse_extra_cors_origins("").is_empty());
    }
}
