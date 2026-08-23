//! Ingest endpoint.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts, State},
    http::{request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use serde_json::json;

use crate::{error_reporting::CapturedError, rate_limiting::action::RateLimitAction};

use super::{
    auth::{authenticate, resolve_client_ip},
    dto::{sanitize, IngestEnvelope, IngestResponse},
    state::CollectorState,
};

/// The socket address a request arrived on, when the server was built with
/// connect info. Never fails: a missing address is a rate-limit bucketing
/// concern, not a reason to refuse a crash report.
pub struct SocketIp(pub Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for SocketIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|info| info.0.ip()),
        ))
    }
}

/// `POST /api/errors` — accept a batch of reports.
///
/// Always answers `202` on an authenticated, parseable request, even when every
/// report was shed. Shedding is a capacity decision, not a client error, and a
/// 5xx here would trigger retry storms from every reporter at exactly the
/// moment the collector is under pressure.
pub async fn ingest<ExtraConfig>(
    State(state): State<CollectorState<ExtraConfig>>,
    SocketIp(socket_ip): SocketIp,
    headers: HeaderMap,
    body: Bytes,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let trust_proxy = state.app.config.rate_limiting.trust_proxy;
    let client_ip = resolve_client_ip(&headers, socket_ip, trust_proxy);

    let Some(identity) = authenticate(&state.config, &headers, client_ip) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_ingest_key" })),
        )
            .into_response();
    };

    // Tiered limiting: the path-based middleware runs before authentication and
    // can only cap by IP, so the identity-aware tier is applied here.
    if let Err(retry_after) = state
        .app
        .rate_limit_state
        .check_rate_limit_key(
            &identity.rate_limit_key,
            &RateLimitAction::new(identity.rate_limit_action),
        )
        .await
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", retry_after.as_secs().to_string())],
            Json(json!({ "error": "rate_limited" })),
        )
            .into_response();
    }

    let envelope: IngestEnvelope = match serde_json::from_slice(&body) {
        Ok(envelope) => envelope,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": "invalid_payload", "detail": e.to_string() })),
            )
                .into_response();
        }
    };

    let limit = state.config.max_events_per_request;
    let total = envelope.events.len();
    let over_limit = total.saturating_sub(limit);
    if over_limit > 0 {
        metrics::counter!("erno_error_reports_dropped_total", "reason" => "per_request_cap")
            .increment(over_limit as u64);
    }

    let ip_string = client_ip.map(|ip| ip.to_string());
    let reports: Vec<CapturedError> = envelope
        .events
        .into_iter()
        .take(limit)
        .map(|event| {
            sanitize(
                event,
                envelope.release.as_deref(),
                envelope.environment.as_deref(),
                envelope.sdk.as_ref(),
                identity.origin,
                ip_string.as_deref(),
                state.config.store_client_ip,
            )
        })
        .collect();

    metrics::counter!(
        "erno_error_reports_received_total",
        "source" => identity.origin.source.as_str()
    )
    .increment(reports.len() as u64);

    let (accepted, sink_dropped) = state
        .sink
        .accept(&state.app.db, &state.config, reports)
        .await;

    (
        StatusCode::ACCEPTED,
        Json(IngestResponse {
            accepted,
            dropped: over_limit + sink_dropped,
        }),
    )
        .into_response()
}
