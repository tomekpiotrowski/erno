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
    // Mock inbox is for local/test only. Production must never expose it, even
    // if someone sets `email.type = "mock"` by mistake.
    let expose_dev_inbox = expose_dev_inbox(&app.config.email, app.environment);

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

    if expose_dev_inbox {
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

fn expose_dev_inbox(email: &EmailConfig, environment: Environment) -> bool {
    matches!(email, EmailConfig::Mock)
        && matches!(
            environment,
            Environment::Development | Environment::Test
        )
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

#[cfg(test)]
mod dev_inbox_tests {
    use axum::Router;
    use axum_test::TestServer;

    use super::{expose_dev_inbox, router};
    use crate::app::App;
    use crate::config::EmailConfig;
    use crate::database::migrations::Migrator;
    use crate::environment::Environment;
    use crate::metrics::collector::CollectorRegistry;
    use crate::metrics::setup_metrics;
    use crate::rate_limiting::RateLimitState;
    use crate::storage::FileStorage;
    use crate::sync::queue::SyncQueue;
    use crate::sync::registry::SyncRegistry;
    use crate::tests::{no_fixtures, setup_test, test_boot};
    use crate::websocket::connections::Connections;
    use std::sync::Arc;

    fn empty_router(_app: App) -> Router {
        Router::new()
    }

    #[test]
    fn mock_inbox_is_dev_and_test_only() {
        assert!(expose_dev_inbox(&EmailConfig::Mock, Environment::Test));
        assert!(expose_dev_inbox(
            &EmailConfig::Mock,
            Environment::Development
        ));
        assert!(!expose_dev_inbox(
            &EmailConfig::Mock,
            Environment::Production
        ));
    }

    #[tokio::test]
    async fn test_environment_serves_the_mock_inbox() {
        let t = setup_test::<Migrator, _>(test_boot(empty_router), no_fixtures).await;
        assert_eq!(t.server.get("/dev/emails").await.status_code(), 200);
    }

    #[tokio::test]
    async fn production_does_not_serve_the_mock_inbox() {
        let t = setup_test::<Migrator, _>(test_boot(empty_router), no_fixtures).await;
        let app = App {
            config: t.config.clone(),
            environment: Environment::Production,
            db: t.db.clone(),
            mailer: t.mailer.clone(),
            job_queue: t.job_queue.clone(),
            sync_queue: SyncQueue::mock(),
            sync_registry: Arc::new(SyncRegistry::new()),
            rate_limit_state: RateLimitState::new(t.config.rate_limiting.clone()),
            websocket_connections: Connections::new(),
            storage: FileStorage::mock(),
            prometheus_handle: setup_metrics(),
            metrics_collectors: Arc::new(CollectorRegistry::default()),
            job_failure_handler: None,
            user_data_deleter: None,
        };
        let server = TestServer::new(router(app, empty_router)).expect("test server");
        assert_eq!(server.get("/dev/emails").await.status_code(), 404);
        assert_eq!(server.get("/dev/jobs").await.status_code(), 404);
    }
}
