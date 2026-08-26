use std::net::{IpAddr, SocketAddr};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use tracing::{debug, instrument, warn};

use super::{action::RateLimitAction, rate_limit_state::RateLimitState};

/// Extension key for storing the rate limit action in request extensions.
///
/// Handlers can insert this into the request to specify which action
/// should be used for rate limiting.
#[derive(Debug, Clone)]
pub struct RateLimitActionExt(pub RateLimitAction);

/// Extract client IP from proxy headers, falling back to the socket address.
///
/// Only reads proxy headers when `trust_proxy` is enabled — otherwise an attacker
/// could spoof `X-Forwarded-For` to bypass rate limiting entirely.
fn extract_client_ip(req: &Request, trust_proxy: bool) -> Option<IpAddr> {
    if trust_proxy {
        // X-Forwarded-For: client, proxy1, proxy2 — leftmost is the real client
        if let Some(ip) = req
            .headers()
            .get("X-Forwarded-For")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
        {
            return Some(ip);
        }

        if let Some(ip) = req
            .headers()
            .get("X-Real-IP")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
        {
            return Some(ip);
        }
    }

    req.extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
}

/// Middleware function that enforces rate limits.
///
/// Extracts the client IP address and rate limit action, then checks
/// if the request should be allowed. Returns 429 Too Many Requests
/// with a Retry-After header if the rate limit is exceeded.
#[instrument(skip(state, req, next), fields(ip, action))]
pub async fn rate_limit_middleware(
    State(state): State<RateLimitState>,
    req: Request,
    next: Next,
) -> Response {
    let ip = match extract_client_ip(&req, state.trust_proxy()) {
        Some(ip) => ip,
        None => {
            warn!("No client IP found in request, allowing request");
            return next.run(req).await;
        }
    };

    tracing::Span::current().record("ip", tracing::field::display(&ip));

    // Get the action from request extensions, or use a default
    let action = req
        .extensions()
        .get::<RateLimitActionExt>()
        .map(|ext| ext.0.clone())
        .unwrap_or_else(|| RateLimitAction::new("default"));

    tracing::Span::current().record("action", action.as_str());

    if action.is_exempt() {
        return next.run(req).await;
    }

    // Check rate limit
    match state.check_rate_limit(ip, &action).await {
        Ok(()) => {
            // Request allowed
            next.run(req).await
        }
        Err(retry_after) => {
            // Rate limit exceeded
            debug!(
                ip = %ip,
                action = action.as_str(),
                retry_after_secs = retry_after.as_secs(),
                "Rate limit exceeded, returning 429"
            );

            Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header(header::RETRY_AFTER, retry_after.as_secs().to_string())
                .body(Body::from("Rate limit exceeded. Please try again later."))
                .unwrap()
        }
    }
}

/// Helper function to create request extensions with a rate limit action.
///
/// This can be used in route-specific middleware to set the action name
/// for rate limiting purposes.
pub fn with_rate_limit_action(action: impl Into<RateLimitAction>) -> RateLimitActionExt {
    RateLimitActionExt(action.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rate_limiting::rate_limit_state::{RateLimitConfig, RateLimitState};
    use axum::extract::ConnectInfo;
    use axum::routing::get;
    use axum::Router;
    use std::collections::HashMap;
    use tower::ServiceExt;

    fn state(max: u32) -> RateLimitState {
        RateLimitState::new(RateLimitConfig {
            enabled: true,
            trust_proxy: false,
            default_window_secs: 60,
            default_max_requests: max,
            backoff_multiplier: 2.0,
            actions: HashMap::new(),
        })
    }

    fn req(path: &str, action: &str) -> Request {
        let mut req = Request::builder().uri(path).body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))));
        req.extensions_mut()
            .insert(RateLimitActionExt(RateLimitAction::new(action)));
        req
    }

    #[tokio::test]
    async fn default_action_is_limited() {
        let app = Router::new().route("/x", get(|| async { "ok" })).layer(
            axum::middleware::from_fn_with_state(state(1), rate_limit_middleware),
        );
        let first = app.clone().oneshot(req("/x", "default")).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let second = app.oneshot(req("/x", "default")).await.unwrap();
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn otlp_auth_is_never_limited() {
        let app = Router::new()
            .route("/api/otlp/auth", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                state(1),
                rate_limit_middleware,
            ));
        for _ in 0..5 {
            let response = app
                .clone()
                .oneshot(req("/api/otlp/auth", RateLimitAction::OTLP_AUTH))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}
