//! End-to-end tests for the outbound reporting path.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! These run against a real HTTP server rather than a mocked client, because
//! the properties worth proving here — that an unreachable collector cannot
//! slow the application down, that a rejected payload is not retried forever —
//! live in the transport, not in the types.

use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use axum::{extract::State, routing::post, Json, Router};
use serde_json::Value;

use crate::{
    app_info::AppInfo,
    environment::Environment,
    error_reporting::{
        config::ErrorReportingConfig, reporter::handle::ErrorReporter, CapturedError, Level, Source,
    },
};

/// A stand-in collector that records what it was sent.
#[derive(Clone, Default)]
struct MockCollector {
    received: Arc<Mutex<Vec<Value>>>,
    requests: Arc<AtomicUsize>,
    status: Arc<AtomicUsize>,
}

impl MockCollector {
    fn new(status: u16) -> Self {
        Self {
            received: Arc::new(Mutex::new(Vec::new())),
            requests: Arc::new(AtomicUsize::new(0)),
            status: Arc::new(AtomicUsize::new(status as usize)),
        }
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    fn envelopes(&self) -> Vec<Value> {
        self.received.lock().expect("not poisoned").clone()
    }
}

async fn collect(
    State(state): State<MockCollector>,
    Json(body): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    state.requests.fetch_add(1, Ordering::SeqCst);
    state.received.lock().expect("not poisoned").push(body);
    let status = axum::http::StatusCode::from_u16(state.status.load(Ordering::SeqCst) as u16)
        .unwrap_or(axum::http::StatusCode::ACCEPTED);
    (
        status,
        Json(serde_json::json!({ "accepted": 1, "dropped": 0 })),
    )
}

/// Start the mock on an ephemeral port and return its base URL.
async fn start_mock(collector: MockCollector) -> String {
    let router = Router::new()
        .route("/api/errors", post(collect))
        .with_state(collector);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{addr}")
}

fn config(collector_url: &str) -> ErrorReportingConfig {
    ErrorReportingConfig {
        collector_url: collector_url.to_string(),
        ingest_token: "server-secret".to_string(),
        batch_size: 10,
        // Short so tests do not wait a second per flush.
        flush_interval_ms: 20,
        request_timeout_ms: 1_000,
        circuit_breaker_failures: 3,
        circuit_breaker_cooldown_ms: 60_000,
        ..ErrorReportingConfig::default()
    }
}

fn report(message: &str) -> CapturedError {
    CapturedError::new(
        Source::Api,
        Level::Error,
        "TestError".to_string(),
        message.to_string(),
    )
}

fn reporter_for(config: &ErrorReportingConfig) -> ErrorReporter {
    ErrorReporter::start(
        config,
        AppInfo::new("test-app", "9.9.9", "under test"),
        Environment::Test,
    )
}

/// Poll until `condition` holds or the deadline passes.
async fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
}

#[tokio::test]
async fn reports_reach_the_collector_with_the_configured_credentials() {
    let mock = MockCollector::new(202);
    let url = start_mock(mock.clone()).await;
    let reporter = reporter_for(&config(&url));

    reporter.capture(report("something broke"));

    assert!(
        wait_for(Duration::from_secs(5), || mock.request_count() > 0).await,
        "the report never arrived"
    );

    let envelopes = mock.envelopes();
    assert_eq!(envelopes[0]["events"][0]["message"], "something broke");
    // The release is the *application's* version, not erno's — that is what an
    // operator needs to correlate an issue with a deploy.
    assert_eq!(envelopes[0]["release"], "9.9.9");
    assert_eq!(envelopes[0]["environment"], "test");
}

#[tokio::test]
async fn reports_are_batched_rather_than_sent_one_by_one() {
    let mock = MockCollector::new(202);
    let url = start_mock(mock.clone()).await;
    let reporter = reporter_for(&config(&url));

    for i in 0..10 {
        reporter.capture(report(&format!("burst {i}")));
    }

    assert!(
        wait_for(Duration::from_secs(5), || {
            mock.envelopes()
                .iter()
                .map(|e| e["events"].as_array().map_or(0, Vec::len))
                .sum::<usize>()
                >= 10
        })
        .await,
        "not all reports arrived"
    );

    assert!(
        mock.request_count() < 10,
        "expected batching, got {} requests for 10 reports",
        mock.request_count()
    );
}

/// The single most important property of the whole design: if the collector is
/// unreachable, the application must not notice.
#[tokio::test]
async fn an_unreachable_collector_never_blocks_the_application() {
    // Port 1 on loopback refuses connections immediately.
    let reporter = reporter_for(&config("http://127.0.0.1:1"));

    let start = Instant::now();
    for i in 0..5_000 {
        reporter.capture(report(&format!("while the collector is down {i}")));
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "capture blocked for {elapsed:?} with the collector down"
    );

    // And it stays responsive afterwards, rather than wedging on retries.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let start = Instant::now();
    reporter.capture(report("still fine"));
    assert!(start.elapsed() < Duration::from_millis(100));
}

/// A 4xx means the payload itself is unacceptable. Retrying it would be a
/// permanent hot loop against a collector that will never accept it.
#[tokio::test]
async fn a_rejected_payload_is_not_retried() {
    let mock = MockCollector::new(400);
    let url = start_mock(mock.clone()).await;
    let reporter = reporter_for(&config(&url));

    reporter.capture(report("malformed as far as the collector is concerned"));

    assert!(
        wait_for(Duration::from_secs(5), || mock.request_count() > 0).await,
        "the report was never attempted"
    );

    // Give any retry ample time to show up.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        mock.request_count(),
        1,
        "a rejected payload must be dropped, not retried"
    );
}

/// A 5xx is transient, so it is worth trying again.
#[tokio::test]
async fn a_collector_side_failure_is_retried() {
    let mock = MockCollector::new(503);
    let url = start_mock(mock.clone()).await;
    let mut config = config(&url);
    // Keep the test quick: the first backoff step is a second.
    config.circuit_breaker_failures = 10;
    let reporter = reporter_for(&config);

    reporter.capture(report("collector is having a bad day"));

    assert!(
        wait_for(Duration::from_secs(8), || mock.request_count() >= 2).await,
        "expected a retry, saw {} request(s)",
        mock.request_count()
    );
}

#[tokio::test]
async fn a_disabled_reporter_makes_no_network_calls_at_all() {
    let mock = MockCollector::new(202);
    let url = start_mock(mock.clone()).await;

    let mut config = config(&url);
    config.enabled = false;
    let reporter = reporter_for(&config);

    reporter.capture(report("should go nowhere"));
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(!reporter.is_active());
    assert_eq!(mock.request_count(), 0);
}
