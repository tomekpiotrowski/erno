//! Request tests for the collector.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! These live in the monitoring crate rather than the library because the
//! framework harness initialises the schema once per test process, so every
//! test in a binary must share one migrator — and only this crate's
//! [`MonitorMigrator`] creates the collector tables.

use axum::Router;
use erno::{
    app::App,
    app_info::AppInfo,
    boot::BootConfig,
    error_reporting::collector::collector_router,
    jobs::job_registry::JobRegistry,
    tests::{no_fixtures, require_single_test_thread, setup_test, TestUtils},
};
use sea_orm::{ConnectionTrait, DbBackend, EntityTrait, Statement};
use serde_json::{json, Value};

use crate::{MonitorConfig, MonitorMigrator};

const BROWSER_TOKEN: &str = "dev-browser-token";
const SERVER_TOKEN: &str = "dev-server-token";
const INGEST: &str = "/api/errors";

fn collector_test_router(app: App<MonitorConfig>) -> Router {
    let config = app.config.extra.collector.clone();
    collector_router(app, config).unwrap_or_default()
}

/// A collector switched off: its routes must not exist at all.
fn disabled_collector_router(app: App<MonitorConfig>) -> Router {
    let mut config = app.config.extra.collector.clone();
    config.enabled = false;
    collector_router(app, config).unwrap_or_default()
}

fn boot(router: fn(App<MonitorConfig>) -> Router) -> BootConfig<MonitorConfig> {
    BootConfig::new(
        AppInfo::new("erno-monitoring-test", "0", ""),
        router,
        JobRegistry::new(),
        vec![],
    )
}

/// Every test in this suite boots through here, so the single-thread guard
/// cannot be skipped by adding a test that mounts its own router.
///
/// See [`require_single_test_thread`]: this suite issues table-wide statements
/// that deadlock against other tests' uncommitted rows when run in parallel.
async fn setup_with(router: fn(App<MonitorConfig>) -> Router) -> TestUtils {
    require_single_test_thread("erno-monitoring");
    setup_test::<MonitorMigrator, MonitorConfig>(boot(router), no_fixtures).await
}

async fn setup() -> TestUtils {
    setup_with(collector_test_router).await
}

/// Named so the failure is greppable, and so the constraint is stated as a test
/// rather than only as a comment in .cargo/config.toml.
#[test]
fn the_monitoring_suite_must_run_single_threaded() {
    require_single_test_thread("erno-monitoring");
}

/// Evaluate a rule the way the runner does.
///
/// The PromQL source is left unconfigured: these tests exercise the database
/// sources, and a rule that queried a Prometheus that is not running would be
/// testing the unavailable path by accident.
async fn observe_rule(
    t: &TestUtils,
    rule: &erno::error_reporting::collector::models::alert_rule::Model,
) -> Result<erno::error_reporting::collector::alerting::evaluator::Observation, sea_orm::DbErr> {
    use erno::error_reporting::collector::alerting::evaluator::{observe, ObserveContext};
    let http = reqwest::Client::new();
    let thresholds = erno::health::HealthThresholds::default();
    observe(
        &ObserveContext {
            db: &t.db,
            thresholds: &thresholds,
            http: &http,
            prometheus_url: None,
        },
        rule,
    )
    .await
}

/// One scalar from the test's own transaction.
async fn scalar(t: &TestUtils, sql: &str) -> i64 {
    t.db.query_one(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .expect("query")
        .expect("a row")
        .try_get::<i64>("", "value")
        .expect("i64 column named value")
}

/// One text column from the test's own transaction.
async fn scalar_text(t: &TestUtils, sql: &str) -> String {
    t.db.query_one(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .expect("query")
        .expect("a row")
        .try_get::<String>("", "value")
        .expect("text column named value")
}

async fn issue_count(t: &TestUtils) -> i64 {
    scalar(t, "SELECT count(*)::bigint AS value FROM error_issue").await
}

async fn event_count(t: &TestUtils) -> i64 {
    scalar(t, "SELECT count(*)::bigint AS value FROM error_event").await
}

fn event(error_type: &str, message: &str) -> Value {
    json!({ "type": error_type, "message": message })
}

fn envelope(events: Vec<Value>) -> Value {
    json!({ "events": events, "release": "1.0.0", "environment": "test" })
}

async fn post_browser(t: &TestUtils, body: &Value) -> axum_test::TestResponse {
    t.server
        .post(INGEST)
        .add_header("x-erno-ingest-key", BROWSER_TOKEN)
        .json(body)
        .await
}

#[tokio::test]
async fn a_valid_report_is_stored_as_one_issue_and_one_event() {
    let t = setup().await;
    let response = post_browser(
        &t,
        &envelope(vec![event("TypeError", "x is not a function")]),
    )
    .await;

    assert_eq!(response.status_code(), 202);
    let body: Value = response.json();
    assert_eq!(body["accepted"], 1);
    assert_eq!(body["dropped"], 0);
    assert_eq!(issue_count(&t).await, 1);
    assert_eq!(event_count(&t).await, 1);
}

#[tokio::test]
async fn the_same_error_twice_is_one_issue_seen_twice() {
    let t = setup().await;
    let body = envelope(vec![event("TypeError", "boom")]);
    post_browser(&t, &body).await;
    post_browser(&t, &body).await;

    assert_eq!(issue_count(&t).await, 1);
    assert_eq!(event_count(&t).await, 2);
    assert_eq!(
        scalar(&t, "SELECT times_seen AS value FROM error_issue").await,
        2
    );
}

/// The property the whole grouping design exists to guarantee: a deploy that
/// shifts line numbers must not mint a new issue.
#[tokio::test]
async fn line_numbers_do_not_split_an_issue() {
    let t = setup().await;
    for line in [12, 987] {
        let body = json!({
            "events": [{
                "type": "TypeError",
                "message": "x is not a function",
                "frames": [{ "function": "Foo", "file": "/src/app/foo.ts", "line": line }]
            }]
        });
        post_browser(&t, &body).await;
    }

    assert_eq!(issue_count(&t).await, 1, "line numbers must not regroup");
    assert_eq!(
        scalar(&t, "SELECT times_seen AS value FROM error_issue").await,
        2
    );
}

#[tokio::test]
async fn distinct_errors_become_distinct_issues() {
    let t = setup().await;
    post_browser(
        &t,
        &envelope(vec![
            event("TypeError", "x is not a function"),
            event("RangeError", "index out of bounds"),
        ]),
    )
    .await;

    assert_eq!(issue_count(&t).await, 2);
}

#[tokio::test]
async fn ingest_requires_a_known_token() {
    let t = setup().await;
    let body = envelope(vec![event("E", "m")]);

    let missing = t.server.post(INGEST).json(&body).await;
    assert_eq!(missing.status_code(), 401);

    let wrong = t
        .server
        .post(INGEST)
        .add_header("x-erno-ingest-key", "not-a-real-token")
        .json(&body)
        .await;
    assert_eq!(wrong.status_code(), 401);

    assert_eq!(issue_count(&t).await, 0);
}

/// The security boundary: the browser token is public, so anything it claims
/// about identity has to be discarded.
#[tokio::test]
async fn a_browser_token_cannot_attribute_an_error_to_a_user() {
    let t = setup().await;
    let body = json!({
        "events": [{
            "type": "SpoofError",
            "message": "pinning this on someone",
            "user": { "id": "550e8400-e29b-41d4-a716-446655440000", "email": "victim@example.com" }
        }]
    });
    let response = t
        .server
        .post(INGEST)
        .add_header("x-erno-ingest-key", BROWSER_TOKEN)
        .add_header("x-erno-source", "api")
        .json(&body)
        .await;

    assert_eq!(response.status_code(), 202);
    assert_eq!(
        scalar(
            &t,
            "SELECT count(*)::bigint AS value FROM error_event WHERE user_id IS NOT NULL"
        )
        .await,
        0,
        "attribution from an untrusted caller must be dropped"
    );
    assert_eq!(
        scalar(
            &t,
            "SELECT count(*)::bigint AS value FROM error_issue WHERE source = 'api'"
        )
        .await,
        0,
        "a browser must never be able to claim the trusted source"
    );
}

#[tokio::test]
async fn the_server_token_is_trusted_with_attribution() {
    let t = setup().await;
    let body = json!({
        "events": [{
            "type": "DbErr",
            "message": "connection refused",
            "user": { "id": "550e8400-e29b-41d4-a716-446655440000", "email": "real@example.com" }
        }]
    });
    let response = t
        .server
        .post(INGEST)
        .add_header("x-erno-ingest-key", SERVER_TOKEN)
        .json(&body)
        .await;

    assert_eq!(response.status_code(), 202);
    assert_eq!(
        scalar(
            &t,
            "SELECT count(*)::bigint AS value FROM error_event WHERE user_email = 'real@example.com'"
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &t,
            "SELECT count(*)::bigint AS value FROM error_issue WHERE source = 'api'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn malformed_json_is_refused_without_storing_anything() {
    let t = setup().await;
    let response = t
        .server
        .post(INGEST)
        .add_header("x-erno-ingest-key", BROWSER_TOKEN)
        .add_header("content-type", "application/json")
        .text("this is not json")
        .await;

    assert_eq!(response.status_code(), 422);
    assert_eq!(issue_count(&t).await, 0);
}

#[tokio::test]
async fn an_over_cap_request_stores_the_cap_and_reports_the_rest_as_dropped() {
    let t = setup().await;
    let events: Vec<Value> = (0..40)
        .map(|i| event("BulkError", &format!("bulk {i}")))
        .collect();
    let response = post_browser(&t, &envelope(events)).await;

    assert_eq!(response.status_code(), 202);
    let body: Value = response.json();
    assert_eq!(body["accepted"], 20, "per-request cap");
    assert_eq!(body["dropped"], 20);
}

/// A runaway loop must cost a bounded number of rows while still being counted
/// in full — the single most important volume control.
#[tokio::test]
async fn the_burst_cap_bounds_stored_rows_but_not_counts() {
    let t = setup().await;
    let events: Vec<Value> = (0..20)
        .map(|_| event("LoopError", "same every time"))
        .collect();
    post_browser(&t, &envelope(events)).await;

    assert_eq!(issue_count(&t).await, 1);
    assert_eq!(
        scalar(&t, "SELECT times_seen AS value FROM error_issue").await,
        20,
        "every occurrence is counted"
    );
    assert_eq!(
        event_count(&t).await,
        10,
        "but only the burst cap is stored"
    );
}

#[tokio::test]
async fn oversized_input_is_truncated_rather_than_refused() {
    let t = setup().await;
    let long = "failed to load record ".repeat(2000);
    let response = post_browser(&t, &envelope(vec![event("TypeError", &long)])).await;

    assert_eq!(
        response.status_code(),
        202,
        "a reporter that gets a 4xx retries forever"
    );
    let stored = scalar(
        &t,
        "SELECT length(message)::bigint AS value FROM error_event LIMIT 1",
    )
    .await;
    assert!(stored <= 4096, "message truncated, got {stored}");
}

#[tokio::test]
async fn a_resolved_issue_reopens_when_it_recurs() {
    let t = setup().await;
    let body = envelope(vec![event("RangeError", "index out of bounds")]);
    post_browser(&t, &body).await;

    // Resolve it an hour ago, in UTC — the column is naive and the collector
    // writes UTC, so a local-time value here would silently never reopen.
    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE error_issue SET status = 'resolved', \
         resolved_at = (now() AT TIME ZONE 'utc') - interval '1 hour'",
    ))
    .await
    .expect("resolve");

    post_browser(&t, &body).await;

    assert_eq!(
        scalar(
            &t,
            "SELECT count(*)::bigint AS value FROM error_issue \
             WHERE status = 'unresolved' AND resolved_at IS NULL"
        )
        .await,
        1,
        "a recurrence after resolution is a regression"
    );
}

#[tokio::test]
async fn an_ignored_issue_stays_ignored_when_it_recurs() {
    let t = setup().await;
    let body = envelope(vec![event("NoiseError", "known and muted")]);
    post_browser(&t, &body).await;

    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE error_issue SET status = 'ignored'",
    ))
    .await
    .expect("ignore");

    post_browser(&t, &body).await;

    assert_eq!(
        scalar(
            &t,
            "SELECT count(*)::bigint AS value FROM error_issue WHERE status = 'ignored'"
        )
        .await,
        1,
        "ignore means ignore"
    );
    assert_eq!(
        scalar(&t, "SELECT times_seen AS value FROM error_issue").await,
        2,
        "but occurrences are still counted"
    );
}

#[tokio::test]
async fn a_disabled_collector_mounts_no_routes() {
    let t = setup_with(disabled_collector_router).await;

    let response = t
        .server
        .post(INGEST)
        .add_header("x-erno-ingest-key", BROWSER_TOKEN)
        .json(&envelope(vec![event("E", "m")]))
        .await;

    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn the_test_config_selects_synchronous_writes() {
    // Guards the whole suite: with a background writer the reports would land
    // outside each test's transaction, leak across tests, and never roll back.
    let config = erno::boot::read_config::<MonitorConfig>(&erno::environment::Environment::Test);
    assert!(
        config.extra.collector.sync_writes,
        "monitoring/config/test.toml must set [collector] sync_writes = true"
    );
    assert_eq!(config.extra.collector.browser_token, BROWSER_TOKEN);
}

// ---------------------------------------------------------------------------
// Operator console API
// ---------------------------------------------------------------------------

const ISSUES: &str = "/api/collector/issues";
/// `admin:admin`, matching the test config's argon2 hash.
const OPERATOR: &str = "Basic YWRtaW46YWRtaW4=";

async fn seed(t: &TestUtils, error_type: &str, message: &str) {
    post_browser(t, &envelope(vec![event(error_type, message)])).await;
}

async fn first_issue_id(t: &TestUtils) -> String {
    let response = t
        .server
        .get(ISSUES)
        .add_header("authorization", OPERATOR)
        .await;
    let body: Value = response.json();
    body["issues"][0]["id"]
        .as_str()
        .expect("an issue id")
        .to_string()
}

#[tokio::test]
async fn the_operator_api_requires_credentials() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;

    let anonymous = t.server.get(ISSUES).await;
    assert_eq!(anonymous.status_code(), 401);

    let wrong = t
        .server
        .get(ISSUES)
        .add_header("authorization", "Basic YWRtaW46bm90LXRoZS1wYXNzd29yZA==")
        .await;
    assert_eq!(wrong.status_code(), 401);
}

#[tokio::test]
async fn issues_are_listed_with_pagination_metadata() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;
    seed(&t, "RangeError", "out of bounds").await;

    let response = t
        .server
        .get(ISSUES)
        .add_header("authorization", OPERATOR)
        .await;
    assert_eq!(response.status_code(), 200);

    let body: Value = response.json();
    assert_eq!(body["total"], 2);
    assert_eq!(body["page"], 1);
    assert_eq!(body["issues"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn the_list_defaults_to_unresolved_and_can_be_filtered() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;
    seed(&t, "RangeError", "out of bounds").await;

    let id = first_issue_id(&t).await;
    t.server
        .post(&format!("{ISSUES}/{id}/resolve"))
        .add_header("authorization", OPERATOR)
        .await;

    let default: Value = t
        .server
        .get(ISSUES)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(
        default["total"], 1,
        "resolved issues leave the default view"
    );

    let all: Value = t
        .server
        .get(&format!("{ISSUES}?status=all"))
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(all["total"], 2);

    let resolved: Value = t
        .server
        .get(&format!("{ISSUES}?status=resolved"))
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(resolved["total"], 1);
}

#[tokio::test]
async fn per_page_is_clamped_so_one_request_cannot_pull_the_whole_table() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;

    let body: Value = t
        .server
        .get(&format!("{ISSUES}?per_page=100000"))
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(body["per_page"], 200);
}

#[tokio::test]
async fn search_matches_title_and_type() {
    let t = setup().await;
    seed(&t, "TypeError", "cannot read property of undefined").await;
    seed(&t, "RangeError", "index out of bounds").await;

    let by_title: Value = t
        .server
        .get(&format!("{ISSUES}?q=undefined"))
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(by_title["total"], 1);

    let by_type: Value = t
        .server
        .get(&format!("{ISSUES}?q=RangeError"))
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(by_type["total"], 1);
}

#[tokio::test]
async fn the_detail_view_carries_events_and_a_stored_count() {
    let t = setup().await;
    let events: Vec<Value> = (0..15)
        .map(|_| event("LoopError", "same every time"))
        .collect();
    post_browser(&t, &envelope(events)).await;

    let id = first_issue_id(&t).await;
    let detail: Value = t
        .server
        .get(&format!("{ISSUES}/{id}"))
        .add_header("authorization", OPERATOR)
        .await
        .json();

    assert_eq!(
        detail["issue"]["times_seen"], 15,
        "every occurrence counted"
    );
    assert_eq!(detail["stored_events"], 10, "but the burst cap bounds rows");
    assert!(detail["latest_event"]["message"].is_string());
    assert_eq!(detail["events"].as_array().unwrap().len(), 10);
}

#[tokio::test]
async fn an_unknown_issue_is_a_404_not_a_500() {
    let t = setup().await;
    let response = t
        .server
        .get(&format!("{ISSUES}/550e8400-e29b-41d4-a716-446655440000"))
        .add_header("authorization", OPERATOR)
        .await;
    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn triage_transitions_round_trip() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;
    let id = first_issue_id(&t).await;

    for (action, expected) in [
        ("resolve", "resolved"),
        ("ignore", "ignored"),
        ("unresolve", "unresolved"),
    ] {
        let body: Value = t
            .server
            .post(&format!("{ISSUES}/{id}/{action}"))
            .add_header("authorization", OPERATOR)
            .await
            .json();
        assert_eq!(body["status"], expected);
    }
}

#[tokio::test]
async fn deleting_an_issue_takes_its_events_with_it() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;
    let id = first_issue_id(&t).await;

    let response = t
        .server
        .delete(&format!("{ISSUES}/{id}"))
        .add_header("authorization", OPERATOR)
        .await;
    assert_eq!(response.status_code(), 204);

    assert_eq!(issue_count(&t).await, 0);
    assert_eq!(event_count(&t).await, 0, "the cascade must remove events");
}

#[tokio::test]
async fn the_series_endpoint_zero_fills_empty_buckets() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;

    let response = t
        .server
        .get("/api/collector/series?hours=24")
        .add_header("authorization", OPERATOR)
        .await;
    assert_eq!(response.status_code(), 200, "body: {}", response.text());
    let body: Value = response.json();

    assert_eq!(body["bucket"], "hour");
    let points = body["points"].as_array().expect("points");
    // 24 hourly buckets plus the current partial one; the exact count depends
    // on where "now" falls, so assert the shape rather than a magic number.
    assert!(
        points.len() >= 24,
        "expected a filled window, got {}",
        points.len()
    );
    assert!(
        points.iter().any(|p| p["count"].as_i64() == Some(0)),
        "quiet buckets must appear as zero, not be omitted"
    );
    assert_eq!(
        points
            .iter()
            .filter_map(|p| p["count"].as_i64())
            .sum::<i64>(),
        1
    );
}

#[tokio::test]
async fn account_deletion_anonymises_events_without_removing_them() {
    let t = setup().await;
    t.server
        .post(INGEST)
        .add_header("x-erno-ingest-key", SERVER_TOKEN)
        .json(&json!({
            "events": [{
                "type": "DbErr",
                "message": "connection refused",
                "user": { "id": "550e8400-e29b-41d4-a716-446655440000", "email": "real@example.com" }
            }]
        }))
        .await;

    let response = t
        .server
        .delete("/api/collector/users/550e8400-e29b-41d4-a716-446655440000/events")
        .add_header("x-erno-ingest-key", SERVER_TOKEN)
        .await;
    assert_eq!(response.status_code(), 200);

    assert_eq!(
        event_count(&t).await,
        1,
        "the diagnostic value of the event is not personal data"
    );
    assert_eq!(
        scalar(
            &t,
            "SELECT count(*)::bigint AS value FROM error_event \
             WHERE user_id IS NULL AND user_email IS NULL"
        )
        .await,
        1,
        "but the identity must be gone"
    );
}

#[tokio::test]
async fn anonymisation_requires_the_trusted_token() {
    let t = setup().await;
    let response = t
        .server
        .delete("/api/collector/users/550e8400-e29b-41d4-a716-446655440000/events")
        .add_header("x-erno-ingest-key", BROWSER_TOKEN)
        .await;
    assert_eq!(
        response.status_code(),
        401,
        "the public browser token must not be able to erase attribution"
    );
}

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

use erno::error_reporting::collector::retention;

#[tokio::test]
async fn retention_removes_aged_events_but_keeps_the_lifetime_count() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;

    // Age the event past any sane retention window.
    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE error_event SET created_at = (now() AT TIME ZONE 'utc') - interval '400 days'",
    ))
    .await
    .expect("age the event");

    let config = erno::error_reporting::CollectorConfig {
        event_retention_days: 30,
        // Keep the issue, so this test isolates event ageing.
        issue_retention_days: 3650,
        ..erno::error_reporting::CollectorConfig::default()
    };
    let outcome = retention::sweep(&t.db, &config).await.expect("sweep");

    assert_eq!(outcome.aged_events, 1);
    assert_eq!(event_count(&t).await, 0);
    assert_eq!(issue_count(&t).await, 1, "the issue itself survives");
    assert_eq!(
        scalar(&t, "SELECT times_seen AS value FROM error_issue").await,
        1,
        "times_seen is a lifetime counter and must not shrink with pruning"
    );
}

#[tokio::test]
async fn retention_trims_an_issue_past_its_event_cap() {
    let t = setup().await;
    // 30 occurrences, but the burst cap already bounds stored rows to 10.
    for _ in 0..3 {
        let events: Vec<Value> = (0..10)
            .map(|_| event("LoopError", "same every time"))
            .collect();
        post_browser(&t, &envelope(events)).await;
    }
    assert_eq!(event_count(&t).await, 30);

    let config = erno::error_reporting::CollectorConfig {
        max_events_per_issue: 5,
        ..erno::error_reporting::CollectorConfig::default()
    };
    let outcome = retention::sweep(&t.db, &config).await.expect("sweep");

    assert_eq!(outcome.capped_events, 25);
    assert_eq!(event_count(&t).await, 5);
    assert_eq!(
        scalar(&t, "SELECT times_seen AS value FROM error_issue").await,
        30,
        "occurrences are still counted in full"
    );
}

#[tokio::test]
async fn retention_removes_stale_issues_and_cascades_their_events() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;

    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE error_issue SET last_seen = (now() AT TIME ZONE 'utc') - interval '400 days'",
    ))
    .await
    .expect("age the issue");

    let config = erno::error_reporting::CollectorConfig {
        // Leave events alone so the cascade is what removes them.
        event_retention_days: 3650,
        issue_retention_days: 90,
        ..erno::error_reporting::CollectorConfig::default()
    };
    let outcome = retention::sweep(&t.db, &config).await.expect("sweep");

    assert_eq!(outcome.stale_issues, 1);
    assert_eq!(issue_count(&t).await, 0);
    assert_eq!(event_count(&t).await, 0, "events cascade with the issue");
}

#[tokio::test]
async fn retention_is_a_no_op_when_nothing_is_old() {
    let t = setup().await;
    seed(&t, "TypeError", "boom").await;

    let outcome = retention::sweep(&t.db, &erno::error_reporting::CollectorConfig::default())
        .await
        .expect("sweep");

    assert_eq!(outcome.aged_events, 0);
    assert_eq!(outcome.capped_events, 0);
    assert_eq!(outcome.stale_issues, 0);
    assert_eq!(event_count(&t).await, 1);
}

// ---------------------------------------------------------------------------
// Release tracking
// ---------------------------------------------------------------------------

const RELEASES: &str = "/api/collector/releases";

async fn record_release(
    t: &TestUtils,
    version: &str,
    environment: &str,
) -> axum_test::TestResponse {
    t.server
        .post(RELEASES)
        .add_header("x-erno-ingest-key", SERVER_TOKEN)
        .json(&json!({
            "version": version,
            "environment": environment,
            "commit_sha": "abc1234",
            "source": "github-actions"
        }))
        .await
}

#[tokio::test]
async fn a_deploy_is_recorded_and_listed() {
    let t = setup().await;
    assert_eq!(
        record_release(&t, "1.0.0", "production")
            .await
            .status_code(),
        201
    );

    let body: Value = t
        .server
        .get(RELEASES)
        .add_header("authorization", OPERATOR)
        .await
        .json();

    assert_eq!(body["releases"].as_array().unwrap().len(), 1);
    assert_eq!(body["releases"][0]["version"], "1.0.0");
    assert_eq!(body["releases"][0]["source"], "github-actions");
}

#[tokio::test]
async fn re_running_a_pipeline_updates_rather_than_duplicates() {
    let t = setup().await;
    record_release(&t, "1.0.0", "production").await;
    record_release(&t, "1.0.0", "production").await;

    let body: Value = t
        .server
        .get(RELEASES)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(body["releases"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_same_version_in_two_environments_is_two_deploys() {
    let t = setup().await;
    record_release(&t, "1.0.0", "staging").await;
    record_release(&t, "1.0.0", "production").await;

    let all: Value = t
        .server
        .get(RELEASES)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(all["releases"].as_array().unwrap().len(), 2);

    let filtered: Value = t
        .server
        .get(&format!("{RELEASES}?environment=production"))
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(filtered["releases"].as_array().unwrap().len(), 1);
}

/// The point of the whole feature: attributing new issues to a deploy.
#[tokio::test]
async fn a_release_reports_how_many_issues_it_introduced() {
    let t = setup().await;
    record_release(&t, "2.0.0", "test").await;

    for error_type in ["TypeError", "RangeError"] {
        post_browser(&t, &envelope(vec![event(error_type, "broke")])).await;
    }

    let body: Value = t
        .server
        .get(RELEASES)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    // `envelope()` stamps release 1.0.0, so 2.0.0 introduced nothing.
    assert_eq!(body["releases"][0]["new_issues"], 0);

    record_release(&t, "1.0.0", "test").await;
    let body: Value = t
        .server
        .get(&format!("{RELEASES}?limit=10"))
        .add_header("authorization", OPERATOR)
        .await
        .json();
    let one = body["releases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["version"] == "1.0.0")
        .expect("1.0.0 listed");
    assert_eq!(one["new_issues"], 2);
}

#[tokio::test]
async fn recording_a_release_requires_the_trusted_token() {
    let t = setup().await;
    let response = t
        .server
        .post(RELEASES)
        .add_header("x-erno-ingest-key", BROWSER_TOKEN)
        .json(&json!({ "version": "1.0.0", "environment": "production" }))
        .await;
    assert_eq!(
        response.status_code(),
        401,
        "the public browser token must not be able to forge deploys"
    );
}

#[tokio::test]
async fn listing_releases_requires_operator_credentials() {
    let t = setup().await;
    assert_eq!(t.server.get(RELEASES).await.status_code(), 401);
}

#[tokio::test]
async fn a_release_without_a_version_is_refused() {
    let t = setup().await;
    let response = t
        .server
        .post(RELEASES)
        .add_header("x-erno-ingest-key", SERVER_TOKEN)
        .json(&json!({ "version": "  ", "environment": "production" }))
        .await;
    assert_eq!(response.status_code(), 422);
}

// ---------------------------------------------------------------------------
// Subsystem health
// ---------------------------------------------------------------------------

const HEALTH: &str = "/api/collector/health";

fn health_snapshot(instance: &str) -> Value {
    json!({
        "instance": instance,
        "release": "1.0.0",
        "environment": "test",
        "reported_at": chrono::Utc::now().naive_utc(),
        "jobs": {
            "pending": 0, "pending_retry": 0, "running": 0,
            "failed_last_hour": 0, "oldest_pending_age_seconds": null, "stuck_running": 0
        },
        "sync": { "push_queue_depth": 0, "oldest_push_age_seconds": null },
        "database": { "pool_size": 5, "pool_idle": 4 },
        "websocket": { "connections": 0 }
    })
}

async fn post_health(t: &TestUtils, body: &Value) -> axum_test::TestResponse {
    t.server
        .post(HEALTH)
        .add_header("x-erno-ingest-key", SERVER_TOKEN)
        .json(body)
        .await
}

#[tokio::test]
async fn a_heartbeat_is_recorded_and_judged_healthy() {
    let t = setup().await;
    assert_eq!(
        post_health(&t, &health_snapshot("api-0"))
            .await
            .status_code(),
        202
    );

    let body: Value = t
        .server
        .get(HEALTH)
        .add_header("authorization", OPERATOR)
        .await
        .json();

    assert_eq!(body["state"], "ok");
    assert_eq!(body["instances"].as_array().unwrap().len(), 1);
    assert_eq!(body["instances"][0]["instance"], "api-0");
    assert!(!body["instances"][0]["stale"].as_bool().unwrap());
    assert_eq!(
        body["instances"][0]["subsystems"].as_array().unwrap().len(),
        4
    );
}

#[tokio::test]
async fn a_heartbeat_replaces_the_previous_reading_for_that_instance() {
    let t = setup().await;
    post_health(&t, &health_snapshot("api-0")).await;
    post_health(&t, &health_snapshot("api-0")).await;

    let body: Value = t
        .server
        .get(HEALTH)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(
        body["instances"].as_array().unwrap().len(),
        1,
        "this is a liveness view, not a time series"
    );
}

#[tokio::test]
async fn replicas_are_tracked_separately() {
    let t = setup().await;
    post_health(&t, &health_snapshot("api-0")).await;
    post_health(&t, &health_snapshot("api-1")).await;

    let body: Value = t
        .server
        .get(HEALTH)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(body["instances"].as_array().unwrap().len(), 2);
}

/// The signal that matters most: a worker died holding a job.
#[tokio::test]
async fn a_stuck_job_makes_the_instance_down() {
    let t = setup().await;
    let mut snapshot = health_snapshot("api-0");
    snapshot["jobs"]["stuck_running"] = json!(2);
    post_health(&t, &snapshot).await;

    let body: Value = t
        .server
        .get(HEALTH)
        .add_header("authorization", OPERATOR)
        .await
        .json();

    assert_eq!(body["state"], "down");
    let jobs = body["instances"][0]["subsystems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "jobs")
        .unwrap();
    assert_eq!(jobs["state"], "down");
    assert!(jobs["detail"]
        .as_str()
        .unwrap()
        .contains("stopped reporting"));
}

#[tokio::test]
async fn a_backed_up_queue_degrades_the_instance() {
    let t = setup().await;
    let mut snapshot = health_snapshot("api-0");
    snapshot["jobs"]["pending"] = json!(400);
    snapshot["jobs"]["oldest_pending_age_seconds"] = json!(300);
    post_health(&t, &snapshot).await;

    let body: Value = t
        .server
        .get(HEALTH)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(body["state"], "degraded");
}

/// A heartbeat that stops is more informative than whatever it last said.
#[tokio::test]
async fn a_silent_instance_is_down_whatever_its_last_reading_claimed() {
    let t = setup().await;
    post_health(&t, &health_snapshot("api-0")).await;

    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE app_health SET reported_at = (now() AT TIME ZONE 'utc') - interval '2 hours'",
    ))
    .await
    .expect("age the heartbeat");

    let body: Value = t
        .server
        .get(HEALTH)
        .add_header("authorization", OPERATOR)
        .await
        .json();

    assert_eq!(body["state"], "down");
    assert!(body["instances"][0]["stale"].as_bool().unwrap());
    assert_eq!(body["instances"][0]["subsystems"][0]["name"], "heartbeat");
}

#[tokio::test]
async fn heartbeats_require_the_trusted_token() {
    let t = setup().await;
    let response = t
        .server
        .post(HEALTH)
        .add_header("x-erno-ingest-key", BROWSER_TOKEN)
        .json(&health_snapshot("api-0"))
        .await;
    assert_eq!(response.status_code(), 401);
}

#[tokio::test]
async fn reading_health_requires_operator_credentials() {
    let t = setup().await;
    assert_eq!(t.server.get(HEALTH).await.status_code(), 401);
}

#[tokio::test]
async fn retired_replicas_are_forgotten() {
    // Without this, every replica a deployment ever had accumulates forever and
    // a rolling deploy quietly doubles the list.
    let t = setup().await;
    post_health(&t, &health_snapshot("api-old")).await;

    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE app_health SET reported_at = (now() AT TIME ZONE 'utc') - interval '30 days'",
    ))
    .await
    .expect("age the heartbeat");

    let removed = erno::error_reporting::collector::health::forget_stale(&t.db, 24 * 60 * 60)
        .await
        .expect("forget");
    assert_eq!(removed, 1);

    let body: Value = t
        .server
        .get(HEALTH)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert!(body["instances"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Uptime checks
// ---------------------------------------------------------------------------

const UPTIME: &str = "/api/collector/uptime";

async fn create_check(t: &TestUtils, name: &str, url: &str) -> axum_test::TestResponse {
    t.server
        .post(UPTIME)
        .add_header("authorization", OPERATOR)
        .json(&json!({ "name": name, "url": url }))
        .await
}

#[tokio::test]
async fn a_check_is_created_and_listed() {
    let t = setup().await;
    assert_eq!(
        create_check(&t, "API liveness", "https://api.example.com/liveness")
            .await
            .status_code(),
        201
    );

    let body: Value = t
        .server
        .get(UPTIME)
        .add_header("authorization", OPERATOR)
        .await
        .json();

    assert_eq!(body["checks"].as_array().unwrap().len(), 1);
    assert_eq!(body["checks"][0]["name"], "API liveness");
    assert_eq!(body["checks"][0]["state"], "unknown");
    // No probes yet is unknown, not zero — an empty check must not look broken.
    assert!(body["checks"][0]["uptime_ratio"].is_null());
}

#[tokio::test]
async fn a_check_needs_a_reachable_looking_url() {
    let t = setup().await;
    for url in ["", "not-a-url", "ftp://example.com"] {
        let response = t
            .server
            .post(UPTIME)
            .add_header("authorization", OPERATOR)
            .json(&json!({ "name": "bad", "url": url }))
            .await;
        assert_eq!(response.status_code(), 422, "url {url:?} should be refused");
    }
}

#[tokio::test]
async fn a_check_needs_a_name() {
    let t = setup().await;
    let response = t
        .server
        .post(UPTIME)
        .add_header("authorization", OPERATOR)
        .json(&json!({ "name": "  ", "url": "https://example.com" }))
        .await;
    assert_eq!(response.status_code(), 422);
}

#[tokio::test]
async fn check_intervals_are_clamped_so_a_typo_cannot_hammer_a_target() {
    let t = setup().await;
    t.server
        .post(UPTIME)
        .add_header("authorization", OPERATOR)
        .json(&json!({
            "name": "aggressive",
            "url": "https://example.com",
            "interval_seconds": 0
        }))
        .await;

    let body: Value = t
        .server
        .get(UPTIME)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(body["checks"][0]["interval_seconds"], 10);
}

#[tokio::test]
async fn a_check_can_be_disabled_and_re_enabled_without_losing_history() {
    let t = setup().await;
    create_check(&t, "toggle me", "https://example.com").await;
    let id = {
        let body: Value = t
            .server
            .get(UPTIME)
            .add_header("authorization", OPERATOR)
            .await
            .json();
        body["checks"][0]["id"].as_str().unwrap().to_string()
    };

    t.server
        .post(&format!("{UPTIME}/{id}/disable"))
        .add_header("authorization", OPERATOR)
        .await;
    let body: Value = t
        .server
        .get(UPTIME)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(body["checks"][0]["enabled"], false);

    t.server
        .post(&format!("{UPTIME}/{id}/enable"))
        .add_header("authorization", OPERATOR)
        .await;
    let body: Value = t
        .server
        .get(UPTIME)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert_eq!(body["checks"][0]["enabled"], true);
}

#[tokio::test]
async fn deleting_a_check_removes_its_results() {
    let t = setup().await;
    create_check(&t, "doomed", "https://example.com").await;
    let id = {
        let body: Value = t
            .server
            .get(UPTIME)
            .add_header("authorization", OPERATOR)
            .await
            .json();
        body["checks"][0]["id"].as_str().unwrap().to_string()
    };

    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "INSERT INTO uptime_result (id, check_id, ok, duration_ms, checked_at)
             VALUES (gen_random_uuid(), '{id}', true, 12, (now() AT TIME ZONE 'utc'))"
        ),
    ))
    .await
    .expect("seed a result");

    let response = t
        .server
        .delete(&format!("{UPTIME}/{id}"))
        .add_header("authorization", OPERATOR)
        .await;
    assert_eq!(response.status_code(), 204);
    assert_eq!(
        scalar(&t, "SELECT count(*)::bigint AS value FROM uptime_result").await,
        0,
        "results cascade with the check"
    );
}

#[tokio::test]
async fn uptime_ratio_and_percentiles_come_from_recorded_probes() {
    let t = setup().await;
    create_check(&t, "measured", "https://example.com").await;
    let id = {
        let body: Value = t
            .server
            .get(UPTIME)
            .add_header("authorization", OPERATOR)
            .await
            .json();
        body["checks"][0]["id"].as_str().unwrap().to_string()
    };

    // 8 successes, 2 failures.
    for i in 0..10 {
        let ok = i >= 2;
        t.db.execute(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "INSERT INTO uptime_result (id, check_id, ok, duration_ms, checked_at)
                 VALUES (gen_random_uuid(), '{id}', {ok}, {}, (now() AT TIME ZONE 'utc'))",
                (i + 1) * 10
            ),
        ))
        .await
        .expect("seed results");
    }

    let body: Value = t
        .server
        .get(UPTIME)
        .add_header("authorization", OPERATOR)
        .await
        .json();
    assert!((body["checks"][0]["uptime_ratio"].as_f64().unwrap() - 0.8).abs() < 1e-9);
    assert_eq!(body["checks"][0]["p50_ms"], 50);
    assert_eq!(body["checks"][0]["p95_ms"], 100);
}

#[tokio::test]
async fn managing_checks_requires_operator_credentials() {
    let t = setup().await;
    assert_eq!(t.server.get(UPTIME).await.status_code(), 401);
    assert_eq!(
        t.server
            .post(UPTIME)
            .json(&json!({ "name": "x", "url": "https://example.com" }))
            .await
            .status_code(),
        401
    );
}

#[tokio::test]
async fn only_due_checks_are_selected_for_probing() {
    use erno::error_reporting::collector::uptime::service;

    let t = setup().await;
    create_check(&t, "never probed", "https://example.com/a").await;
    create_check(&t, "just probed", "https://example.com/b").await;
    create_check(&t, "long overdue", "https://example.com/c").await;

    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE uptime_check SET last_checked_at = (now() AT TIME ZONE 'utc')
         WHERE name = 'just probed'",
    ))
    .await
    .expect("mark just probed");
    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE uptime_check SET last_checked_at = (now() AT TIME ZONE 'utc') - interval '1 hour'
         WHERE name = 'long overdue'",
    ))
    .await
    .expect("mark overdue");

    let due = service::due(&t.db).await.expect("due");
    let names: Vec<&str> = due.iter().map(|c| c.name.as_str()).collect();

    assert!(names.contains(&"never probed"));
    assert!(names.contains(&"long overdue"));
    assert!(
        !names.contains(&"just probed"),
        "a check inside its interval is not due"
    );
}

#[tokio::test]
async fn a_disabled_check_is_never_due() {
    use erno::error_reporting::collector::uptime::service;

    let t = setup().await;
    create_check(&t, "off", "https://example.com").await;
    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE uptime_check SET enabled = false",
    ))
    .await
    .expect("disable");

    assert!(service::due(&t.db).await.expect("due").is_empty());
}

/// Two failures take it down; one success brings it straight back.
#[tokio::test]
async fn probes_move_the_check_state_with_flap_damping() {
    use erno::error_reporting::collector::uptime::{service, ProbeOutcome};

    let t = setup().await;
    create_check(&t, "flappy", "https://example.com").await;

    let check = service::due(&t.db)
        .await
        .expect("due")
        .into_iter()
        .next()
        .expect("the new check is due immediately");
    let changed =
        service::record_probe(&t.db, &check, &ProbeOutcome::failure(Some(500), 5, "boom"))
            .await
            .expect("record");
    assert!(!changed, "one failure must not take a check down");

    let state = scalar_text(&t, "SELECT current_state AS value FROM uptime_check").await;
    assert_eq!(state, "unknown");

    // Second failure crosses the threshold.
    let check = erno::error_reporting::collector::models::uptime_check::Entity::find()
        .one(&t.db)
        .await
        .expect("query")
        .expect("row");
    let changed =
        service::record_probe(&t.db, &check, &ProbeOutcome::failure(Some(500), 5, "boom"))
            .await
            .expect("record");
    assert!(changed, "the threshold failure is the transition");
    assert_eq!(
        scalar_text(&t, "SELECT current_state AS value FROM uptime_check").await,
        "down"
    );

    // A single success is believed immediately.
    let check = erno::error_reporting::collector::models::uptime_check::Entity::find()
        .one(&t.db)
        .await
        .expect("query")
        .expect("row");
    let changed = service::record_probe(&t.db, &check, &ProbeOutcome::success(200, 7))
        .await
        .expect("record");
    assert!(changed);
    assert_eq!(
        scalar_text(&t, "SELECT current_state AS value FROM uptime_check").await,
        "up"
    );
    assert_eq!(
        scalar(&t, "SELECT count(*)::bigint AS value FROM uptime_result").await,
        3
    );
}

#[tokio::test]
async fn old_probe_results_are_pruned() {
    use erno::error_reporting::collector::uptime::service;

    let t = setup().await;
    create_check(&t, "prunable", "https://example.com").await;
    let id = scalar_text(&t, "SELECT id::text AS value FROM uptime_check").await;

    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "INSERT INTO uptime_result (id, check_id, ok, duration_ms, checked_at)
             VALUES (gen_random_uuid(), '{id}', true, 10, (now() AT TIME ZONE 'utc') - interval '30 days')"
        ),
    ))
    .await
    .expect("seed old result");

    assert_eq!(service::prune_results(&t.db, 7).await.expect("prune"), 1);
    assert_eq!(
        scalar(&t, "SELECT count(*)::bigint AS value FROM uptime_result").await,
        0
    );
}

// ---------------------------------------------------------------------------
// Status page
// ---------------------------------------------------------------------------

const COMPONENTS: &str = "/api/collector/status/components";
const INCIDENTS: &str = "/api/collector/status/incidents";
const SNAPSHOT: &str = "/api/collector/status.json";

async fn create_component(t: &TestUtils, name: &str, check_id: Option<&str>) -> String {
    let mut body = json!({ "name": name });
    if let Some(id) = check_id {
        body["auto_from_check_id"] = json!(id);
    }
    let response = t
        .server
        .post(COMPONENTS)
        .add_header("authorization", OPERATOR)
        .json(&body)
        .await;
    assert_eq!(response.status_code(), 201);
    response.json::<Value>()["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn the_public_snapshot_needs_no_credentials() {
    // It is a preview of a public document; requiring auth would be wrong.
    let t = setup().await;
    let response = t.server.get(SNAPSHOT).await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.json::<Value>()["state"], "operational");
}

#[tokio::test]
async fn an_empty_status_page_is_operational_not_broken() {
    let t = setup().await;
    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert_eq!(body["state"], "operational");
    assert!(body["components"].as_array().unwrap().is_empty());
    assert!(body["active_incidents"].as_array().unwrap().is_empty());
    // The page needs this to judge whether what it is showing can be trusted.
    assert!(body["refresh_seconds"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn a_manual_component_follows_the_operator() {
    let t = setup().await;
    let id = create_component(&t, "Website", None).await;

    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert_eq!(body["components"][0]["state"], "operational");

    t.server
        .post(&format!("{COMPONENTS}/{id}/state"))
        .add_header("authorization", OPERATOR)
        .json(&json!({ "state": "major_outage" }))
        .await;

    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert_eq!(body["components"][0]["state"], "major_outage");
    assert_eq!(
        body["state"], "major_outage",
        "one component, all of it out"
    );
}

#[tokio::test]
async fn a_component_bound_to_a_check_follows_the_probe() {
    let t = setup().await;
    create_check(&t, "API", "https://api.example.com/liveness").await;
    let check_id = scalar_text(&t, "SELECT id::text AS value FROM uptime_check").await;
    create_component(&t, "API", Some(&check_id)).await;

    // A check that has never reported must not be announced as an outage.
    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert_eq!(body["components"][0]["state"], "operational");

    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE uptime_check SET current_state = 'down'",
    ))
    .await
    .expect("mark down");

    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert_eq!(body["components"][0]["state"], "major_outage");
}

#[tokio::test]
async fn one_component_out_of_several_is_a_partial_outage() {
    let t = setup().await;
    let id = create_component(&t, "API", None).await;
    create_component(&t, "Website", None).await;

    t.server
        .post(&format!("{COMPONENTS}/{id}/state"))
        .add_header("authorization", OPERATOR)
        .json(&json!({ "state": "major_outage" }))
        .await;

    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert_eq!(body["state"], "partial_outage");
}

#[tokio::test]
async fn component_uptime_history_comes_from_probe_results() {
    let t = setup().await;
    create_check(&t, "API", "https://api.example.com/liveness").await;
    let check_id = scalar_text(&t, "SELECT id::text AS value FROM uptime_check").await;
    create_component(&t, "API", Some(&check_id)).await;

    for (days_ago, ok) in [(2, true), (2, true), (1, true), (1, false)] {
        t.db.execute(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "INSERT INTO uptime_result (id, check_id, ok, duration_ms, checked_at)
                 VALUES (gen_random_uuid(), '{check_id}', {ok}, 10,
                         (now() AT TIME ZONE 'utc') - interval '{days_ago} days')"
            ),
        ))
        .await
        .expect("seed results");
    }

    let body: Value = t.server.get(SNAPSHOT).await.json();
    let history = body["components"][0]["history"].as_array().unwrap();
    assert_eq!(history.len(), 2, "one entry per measured day");
    assert!((body["components"][0]["uptime_ratio"].as_f64().unwrap() - 0.75).abs() < 1e-9);
}

#[tokio::test]
async fn an_incident_appears_with_its_timeline_and_then_resolves() {
    let t = setup().await;
    let component_id = create_component(&t, "API", None).await;

    let opened = t
        .server
        .post(INCIDENTS)
        .add_header("authorization", OPERATOR)
        .json(&json!({
            "title": "Elevated error rates",
            "impact": "major",
            "component_ids": [component_id],
            "body": "We are investigating elevated error rates on the API."
        }))
        .await;
    assert_eq!(opened.status_code(), 201);
    let incident_id = opened.json::<Value>()["id"].as_str().unwrap().to_string();

    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert_eq!(body["active_incidents"].as_array().unwrap().len(), 1);
    assert_eq!(body["active_incidents"][0]["status"], "investigating");
    assert_eq!(
        body["active_incidents"][0]["updates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    t.server
        .post(&format!("{INCIDENTS}/{incident_id}/updates"))
        .add_header("authorization", OPERATOR)
        .json(&json!({ "status": "identified", "body": "A bad deploy. Rolling back." }))
        .await;

    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert_eq!(body["active_incidents"][0]["status"], "identified");
    assert_eq!(
        body["active_incidents"][0]["updates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    t.server
        .post(&format!("{INCIDENTS}/{incident_id}/updates"))
        .add_header("authorization", OPERATOR)
        .json(&json!({ "status": "resolved", "body": "The rollback completed." }))
        .await;

    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert!(
        body["active_incidents"].as_array().unwrap().is_empty(),
        "a resolved incident leaves the active list"
    );
    assert_eq!(body["recent_incidents"].as_array().unwrap().len(), 1);
    assert!(body["recent_incidents"][0]["resolved_at"].is_string());
}

#[tokio::test]
async fn an_incident_needs_a_title_and_a_first_update() {
    let t = setup().await;
    for body in [
        json!({ "title": "", "body": "something" }),
        json!({ "title": "Something", "body": "  " }),
    ] {
        let response = t
            .server
            .post(INCIDENTS)
            .add_header("authorization", OPERATOR)
            .json(&body)
            .await;
        assert_eq!(response.status_code(), 422);
    }
}

#[tokio::test]
async fn an_unknown_impact_falls_back_to_minor_rather_than_alarming_people() {
    let t = setup().await;
    t.server
        .post(INCIDENTS)
        .add_header("authorization", OPERATOR)
        .json(&json!({ "title": "Odd", "impact": "catastrophic", "body": "Looking into it." }))
        .await;

    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert_eq!(body["active_incidents"][0]["impact"], "minor");
}

#[tokio::test]
async fn managing_the_status_page_requires_operator_credentials() {
    let t = setup().await;
    assert_eq!(
        t.server
            .post(COMPONENTS)
            .json(&json!({ "name": "x" }))
            .await
            .status_code(),
        401
    );
    assert_eq!(
        t.server
            .post(INCIDENTS)
            .json(&json!({ "title": "x", "body": "y" }))
            .await
            .status_code(),
        401
    );
    assert_eq!(t.server.get(COMPONENTS).await.status_code(), 401);
}

#[tokio::test]
async fn deleting_a_component_removes_it_from_the_page() {
    let t = setup().await;
    let id = create_component(&t, "Temporary", None).await;

    let response = t
        .server
        .delete(&format!("{COMPONENTS}/{id}"))
        .add_header("authorization", OPERATOR)
        .await;
    assert_eq!(response.status_code(), 204);

    let body: Value = t.server.get(SNAPSHOT).await.json();
    assert!(body["components"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn the_publisher_writes_a_document_that_the_page_can_read() {
    use erno::error_reporting::collector::status::publisher;
    use erno::error_reporting::StatusConfig;

    let t = setup().await;
    create_component(&t, "API", None).await;

    let dir = std::env::temp_dir().join(format!("erno-status-test-{}", uuid::Uuid::new_v4()));
    let path = dir.join("status.json");
    let config = StatusConfig {
        enabled: true,
        name: "Acme status".to_string(),
        output_path: path.to_string_lossy().to_string(),
        refresh_seconds: 30,
    };

    publisher::publish_once(&t.db, &config)
        .await
        .expect("publish");

    let written = std::fs::read_to_string(&path).expect("the document exists");
    let snapshot: Value = serde_json::from_str(&written).expect("it is valid JSON");
    assert_eq!(snapshot["name"], "Acme status");
    assert_eq!(snapshot["state"], "operational");
    assert_eq!(snapshot["components"][0]["name"], "API");
    assert_eq!(snapshot["refresh_seconds"], 30);

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Alert rules
// ---------------------------------------------------------------------------

const ALERTS: &str = "/api/collector/alerts";

async fn create_rule(t: &TestUtils, body: Value) -> axum_test::TestResponse {
    t.server
        .post(ALERTS)
        .add_header("authorization", OPERATOR)
        .json(&body)
        .await
}

async fn list_rules(t: &TestUtils) -> Value {
    t.server
        .get(ALERTS)
        .add_header("authorization", OPERATOR)
        .await
        .json()
}

#[tokio::test]
async fn a_rule_is_created_and_listed() {
    let t = setup().await;
    let response = create_rule(
        &t,
        json!({
            "name": "New error types",
            "source": "errors",
            "selector": "new_issues",
            "threshold": 0,
            "for_seconds": 0
        }),
    )
    .await;
    assert_eq!(response.status_code(), 201);

    let body = list_rules(&t).await;
    assert_eq!(body["rules"].as_array().unwrap().len(), 1);
    assert_eq!(body["rules"][0]["state"], "ok");
    assert_eq!(body["rules"][0]["severity"], "warning");
}

#[tokio::test]
async fn a_rule_needs_a_known_source() {
    let t = setup().await;
    let response = create_rule(
        &t,
        json!({ "name": "Nonsense", "source": "telepathy", "threshold": 1 }),
    )
    .await;
    assert_eq!(response.status_code(), 422);
}

#[tokio::test]
async fn a_rule_needs_a_name() {
    let t = setup().await;
    let response = create_rule(
        &t,
        json!({ "name": "   ", "source": "errors", "threshold": 1 }),
    )
    .await;
    assert_eq!(response.status_code(), 422);
}

/// The alert Alertmanager structurally cannot express.
#[tokio::test]
async fn an_errors_rule_fires_on_new_issue_types() {
    use erno::error_reporting::collector::alerting::service;

    let t = setup().await;
    create_rule(
        &t,
        json!({
            "name": "New error types",
            "source": "errors",
            "selector": "new_issues",
            "threshold": 1,
            "window_seconds": 3600
        }),
    )
    .await;

    let rule = service::enabled(&t.db).await.expect("rules").remove(0);

    let quiet = observe_rule(&t, &rule).await.expect("observe");
    assert_eq!(quiet.value, 0.0);

    post_browser(&t, &envelope(vec![event("TypeError", "boom")])).await;
    post_browser(&t, &envelope(vec![event("RangeError", "out of bounds")])).await;

    let noisy = observe_rule(&t, &rule).await.expect("observe");
    assert_eq!(noisy.value, 2.0);
    assert!(noisy.description.contains("new error type"));
}

#[tokio::test]
async fn an_errors_rule_can_count_event_volume_by_source() {
    use erno::error_reporting::collector::alerting::service;

    let t = setup().await;
    create_rule(
        &t,
        json!({
            "name": "App error volume",
            "source": "errors",
            "selector": "app",
            "threshold": 10,
            "window_seconds": 3600
        }),
    )
    .await;

    post_browser(&t, &envelope(vec![event("TypeError", "boom")])).await;
    t.server
        .post(INGEST)
        .add_header("x-erno-ingest-key", SERVER_TOKEN)
        .json(&json!({ "events": [{ "type": "DbErr", "message": "server side" }] }))
        .await;

    let rule = service::enabled(&t.db).await.expect("rules").remove(0);
    let observed = observe_rule(&t, &rule).await.expect("observe");

    assert_eq!(observed.value, 1.0, "only the app-sourced event counts");
}

#[tokio::test]
async fn an_uptime_rule_counts_checks_that_are_down() {
    use erno::error_reporting::collector::alerting::service;

    let t = setup().await;
    create_check(&t, "API", "https://api.example.com/liveness").await;
    create_rule(
        &t,
        json!({ "name": "Anything down", "source": "uptime", "threshold": 0 }),
    )
    .await;

    let rule = service::enabled(&t.db).await.expect("rules").remove(0);
    assert_eq!(observe_rule(&t, &rule).await.expect("observe").value, 0.0);

    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE uptime_check SET current_state = 'down'",
    ))
    .await
    .expect("mark down");

    assert_eq!(observe_rule(&t, &rule).await.expect("observe").value, 1.0);
}

#[tokio::test]
async fn a_subsystem_rule_counts_unhealthy_instances() {
    use erno::error_reporting::collector::alerting::service;

    let t = setup().await;
    create_rule(
        &t,
        json!({ "name": "Instances down", "source": "subsystem", "threshold": 0 }),
    )
    .await;
    let rule = service::enabled(&t.db).await.expect("rules").remove(0);

    post_health(&t, &health_snapshot("api-0")).await;
    assert_eq!(observe_rule(&t, &rule).await.expect("observe").value, 0.0);

    let mut broken = health_snapshot("api-1");
    broken["jobs"]["stuck_running"] = json!(3);
    post_health(&t, &broken).await;

    assert_eq!(observe_rule(&t, &rule).await.expect("observe").value, 1.0);
}

#[tokio::test]
async fn an_unknown_source_reads_as_nothing_wrong_rather_than_firing() {
    use erno::error_reporting::collector::alerting::service;

    let t = setup().await;
    create_rule(
        &t,
        json!({ "name": "Fine", "source": "errors", "threshold": 1 }),
    )
    .await;

    // Corrupt the stored source, as a bad migration or hand edit might.
    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "UPDATE alert_rule SET source = 'telepathy'",
    ))
    .await
    .expect("corrupt");

    let rule = service::enabled(&t.db).await.expect("rules").remove(0);
    let observed = observe_rule(&t, &rule).await.expect("observe");
    assert_eq!(observed.value, 0.0, "a typo must not page anyone");
}

#[tokio::test]
async fn a_rule_can_be_disabled_silenced_and_deleted() {
    let t = setup().await;
    create_rule(
        &t,
        json!({ "name": "Toggle", "source": "errors", "threshold": 1 }),
    )
    .await;
    let id = list_rules(&t).await["rules"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    t.server
        .post(&format!("{ALERTS}/{id}/disable"))
        .add_header("authorization", OPERATOR)
        .await;
    assert_eq!(list_rules(&t).await["rules"][0]["enabled"], false);

    t.server
        .post(&format!("{ALERTS}/{id}/enable"))
        .add_header("authorization", OPERATOR)
        .await;
    assert_eq!(list_rules(&t).await["rules"][0]["enabled"], true);

    let silenced = t
        .server
        .post(&format!("{ALERTS}/{id}/silence"))
        .add_header("authorization", OPERATOR)
        .json(&json!({ "minutes": 60 }))
        .await;
    assert!(silenced.json::<Value>()["silence_until"].is_string());

    // Zero clears it rather than silencing for no time.
    let cleared = t
        .server
        .post(&format!("{ALERTS}/{id}/silence"))
        .add_header("authorization", OPERATOR)
        .json(&json!({ "minutes": 0 }))
        .await;
    assert!(cleared.json::<Value>()["silence_until"].is_null());

    let deleted = t
        .server
        .delete(&format!("{ALERTS}/{id}"))
        .add_header("authorization", OPERATOR)
        .await;
    assert_eq!(deleted.status_code(), 204);
    assert!(list_rules(&t).await["rules"].as_array().unwrap().is_empty());
}

/// The whole loop: breach, hold, fire, recover — with a real mailer.
#[tokio::test]
async fn the_evaluator_moves_a_rule_through_its_lifecycle_and_notifies() {
    use erno::error_reporting::collector::alerting::{notifier::NotifyContext, runner};
    use erno::health::HealthThresholds;

    let t = setup().await;
    create_rule(
        &t,
        json!({
            "name": "Any new error type",
            "source": "errors",
            "selector": "new_issues",
            "threshold": 0,
            "window_seconds": 3600,
            "for_seconds": 0,
            "repeat_seconds": 0,
            "notify_email": "ops@example.com"
        }),
    )
    .await;

    let client = reqwest::Client::new();
    let context = NotifyContext {
        sender: "noreply@example.com".to_string(),
        default_recipient: None,
        console_url: "http://localhost:4400".to_string(),
    };
    let thresholds = HealthThresholds::default();

    // Nothing has happened yet.
    runner::evaluate_all(&t.db, &t.mailer, &client, &context, &thresholds, None)
        .await
        .expect("evaluate");
    assert_eq!(list_rules(&t).await["rules"][0]["state"], "ok");
    assert!(t.sent_emails().is_empty());

    // An error appears; `for_seconds` is zero so it is believed immediately.
    post_browser(&t, &envelope(vec![event("TypeError", "boom")])).await;
    runner::evaluate_all(&t.db, &t.mailer, &client, &context, &thresholds, None)
        .await
        .expect("evaluate");

    assert_eq!(list_rules(&t).await["rules"][0]["state"], "firing");
    let sent = t.sent_emails();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].subject.contains("FIRING"));
    assert_eq!(sent[0].to, "ops@example.com");

    // Still firing, but `repeat_seconds` is zero: notify once only.
    runner::evaluate_all(&t.db, &t.mailer, &client, &context, &thresholds, None)
        .await
        .expect("evaluate");
    assert_eq!(
        t.sent_emails().len(),
        1,
        "a firing rule must not mail on every evaluation"
    );

    // Clear the errors; the rule recovers and says so.
    t.db.execute(Statement::from_string(
        DbBackend::Postgres,
        "TRUNCATE error_issue CASCADE",
    ))
    .await
    .expect("clear");

    runner::evaluate_all(&t.db, &t.mailer, &client, &context, &thresholds, None)
        .await
        .expect("evaluate");

    assert_eq!(list_rules(&t).await["rules"][0]["state"], "ok");
    let sent = t.sent_emails();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].subject.contains("RESOLVED"));
}

#[tokio::test]
async fn a_silenced_rule_still_tracks_state_but_sends_nothing() {
    use erno::error_reporting::collector::alerting::{notifier::NotifyContext, runner};
    use erno::health::HealthThresholds;

    let t = setup().await;
    create_rule(
        &t,
        json!({
            "name": "Silenced",
            "source": "errors",
            "selector": "new_issues",
            "threshold": 0,
            "for_seconds": 0,
            "notify_email": "ops@example.com"
        }),
    )
    .await;
    let id = list_rules(&t).await["rules"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    t.server
        .post(&format!("{ALERTS}/{id}/silence"))
        .add_header("authorization", OPERATOR)
        .json(&json!({ "minutes": 60 }))
        .await;

    post_browser(&t, &envelope(vec![event("TypeError", "boom")])).await;
    runner::evaluate_all(
        &t.db,
        &t.mailer,
        &reqwest::Client::new(),
        &NotifyContext {
            sender: "noreply@example.com".to_string(),
            default_recipient: None,
            console_url: "http://localhost:4400".to_string(),
        },
        &HealthThresholds::default(),
        None,
    )
    .await
    .expect("evaluate");

    assert_eq!(
        list_rules(&t).await["rules"][0]["state"],
        "firing",
        "the console still shows the truth"
    );
    assert!(t.sent_emails().is_empty(), "but nobody is told");
}

#[tokio::test]
async fn managing_alert_rules_requires_operator_credentials() {
    let t = setup().await;
    assert_eq!(t.server.get(ALERTS).await.status_code(), 401);
    assert_eq!(
        t.server
            .post(ALERTS)
            .json(&json!({ "name": "x", "source": "errors", "threshold": 1 }))
            .await
            .status_code(),
        401
    );
}
