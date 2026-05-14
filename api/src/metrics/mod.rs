//! Docs: docs/src/content/docs/api/telemetry.md
pub mod collector;
pub mod db_stats;
pub mod http;

pub use collector::{CollectorRegistry, MetricsCollector};
pub use metrics_exporter_prometheus::PrometheusHandle;

#[derive(Clone)]
pub struct MetricsEndpointState {
    pub handle: PrometheusHandle,
    pub auth_token: Option<String>,
}

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::OnceLock;

static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the Prometheus recorder once and return a handle.  Safe to call
/// from tests or multiple code paths — subsequent calls return the same handle.
pub fn setup_metrics() -> PrometheusHandle {
    PROMETHEUS_HANDLE
        .get_or_init(|| {
            PrometheusBuilder::new()
                .install_recorder()
                .expect("failed to install Prometheus metrics recorder")
        })
        .clone()
}

pub async fn metrics_handler(
    State(state): State<MetricsEndpointState>,
    headers: HeaderMap,
) -> Response {
    if let Some(expected) = &state.auth_token {
        let provided = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        if provided != Some(expected.as_str()) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

    state.handle.render().into_response()
}
