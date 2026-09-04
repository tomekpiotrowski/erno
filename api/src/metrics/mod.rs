//! Docs: docs/src/content/docs/api/telemetry.md
pub mod collector;
pub mod db_stats;
pub mod http;
pub mod otlp_push;
pub mod timing;

pub use collector::{CollectorRegistry, MetricsCollector};
pub use metrics_exporter_prometheus::PrometheusHandle;
pub use timing::OperationTimer;

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
            // Buckets, not the crate's default summaries: precomputed
            // quantiles cannot be aggregated across instances or windows, and
            // the collector's rollups fold bucket vectors. Seconds-shaped
            // buckets by default; row-count buckets for the sync-delta
            // histogram, which measures rows, not time.
            PrometheusBuilder::new()
                .set_buckets(&[
                    0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
                    30.0,
                ])
                .expect("static buckets")
                .set_buckets_for_metric(
                    metrics_exporter_prometheus::Matcher::Suffix("_rows".to_string()),
                    &[1.0, 10.0, 100.0, 1_000.0, 10_000.0, 100_000.0],
                )
                .expect("static buckets")
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
