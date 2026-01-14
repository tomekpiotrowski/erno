use std::net::SocketAddr;

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
    // Extract IP from ConnectInfo extension (added by the server)
    let ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0.ip());

    let ip = match ip {
        Some(ip) => ip,
        None => {
            warn!("No ConnectInfo found in request, allowing request");
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

    // Check rate limit
    match state.check_rate_limit(ip, &action) {
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
