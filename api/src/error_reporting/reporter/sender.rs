//! Outbound delivery to a collector.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::error_reporting::CapturedError;

use super::{backoff, circuit_breaker::CircuitBreaker};

/// Attempts for one batch before it is abandoned.
///
/// Bounded on purpose: retrying forever would stall the drain loop, back the
/// queue up, and turn a collector outage into report loss anyway — just later
/// and less visibly.
const MAX_ATTEMPTS: u32 = 4;

/// Everything the sender task needs.
#[derive(Debug, Clone)]
pub struct SenderConfig {
    /// Absolute ingest URL.
    pub endpoint: String,
    /// Trusted server-to-server token.
    pub token: String,
    /// Reports per outbound request.
    pub batch_size: usize,
    /// How long to accumulate before sending.
    pub flush_interval: Duration,
    /// Hard cap on one request, so a black-holed collector cannot pin the task.
    pub request_timeout: Duration,
    /// Consecutive failures before the breaker opens.
    pub circuit_breaker_failures: u32,
    /// How long the breaker stays open.
    pub circuit_breaker_cooldown: Duration,
    /// Release stamped on every report.
    pub release: Option<String>,
    /// Environment stamped on every report.
    pub environment: Option<String>,
}

/// What to do about a delivery attempt's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Delivered.
    Delivered,
    /// Worth another attempt.
    Retry,
    /// The collector rejected the payload itself; retrying cannot help.
    Discard,
}

/// Decide what a response status means for delivery.
///
/// The important asymmetry: a 4xx other than 429 means *this payload* is
/// unacceptable, so retrying it would loop forever against a collector that
/// will never accept it. Everything else is treated as transient.
#[must_use]
pub const fn classify_status(status: u16) -> Disposition {
    match status {
        200..=299 => Disposition::Delivered,
        // Explicitly transient.
        429 => Disposition::Retry,
        // The payload is the problem — a retry would be a permanent hot loop.
        400..=499 => Disposition::Discard,
        // Collector-side trouble; back off and try again.
        _ => Disposition::Retry,
    }
}

/// One report as an OTLP log record.
///
/// The collector groups on `exception.type` + `erno.frames` (lossless,
/// already-parsed) and honours `erno.fingerprint`; everything else in the
/// context rides as plain attributes. `code.filepath` carries the call site
/// the way the semconv spells it, so a stackless `tracing::error!` still
/// groups by where it was written.
fn to_log_record(report: &CapturedError) -> opentelemetry_proto::tonic::logs::v1::LogRecord {
    use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};

    let attr = |key: &str, value: String| KeyValue {
        key: key.to_string(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value)),
        }),
    };
    let mut attributes = vec![
        attr("exception.type", report.error_type.clone()),
        attr("erno.source", report.source.as_str().to_string()),
    ];
    if let Some(stack) = &report.stack {
        attributes.push(attr("exception.stacktrace", stack.clone()));
    }
    if !report.frames.is_empty() {
        if let Ok(frames) = serde_json::to_string(&report.frames) {
            attributes.push(attr("erno.frames", frames));
        }
    }
    if let Some(fingerprint) = &report.client_fingerprint {
        if let Ok(fingerprint) = serde_json::to_string(fingerprint) {
            attributes.push(attr("erno.fingerprint", fingerprint));
        }
    }
    if let Some(user_id) = report.user_id {
        attributes.push(attr("enduser.id", user_id.to_string()));
    }
    if let Some(email) = &report.user_email {
        attributes.push(attr("user.email", email.clone()));
    }
    if let Some(object) = report.context.as_object() {
        for (key, value) in object {
            let key = if key == "file" {
                "code.filepath"
            } else {
                key.as_str()
            };
            let value = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            attributes.push(attr(key, value));
        }
    }

    let severity = match report.level {
        crate::error_reporting::Level::Warning => 13,
        crate::error_reporting::Level::Error => 17,
        crate::error_reporting::Level::Fatal => 21,
    };
    #[allow(clippy::cast_sign_loss)]
    let nanos = report
        .timestamp
        .and_utc()
        .timestamp_nanos_opt()
        .unwrap_or(0)
        .max(0) as u64;
    opentelemetry_proto::tonic::logs::v1::LogRecord {
        time_unix_nano: nanos,
        observed_time_unix_nano: nanos,
        severity_number: severity,
        severity_text: report.level.as_str().to_string(),
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue(report.message.clone())),
        }),
        attributes,
        ..Default::default()
    }
}

/// The OTLP request body for one batch.
fn build_export_request(
    reports: &[CapturedError],
    config: &SenderConfig,
) -> opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest {
    use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};

    let mut resource_attributes = Vec::new();
    let mut resource_attr = |key: &str, value: &str| {
        resource_attributes.push(KeyValue {
            key: key.to_string(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(value.to_string())),
            }),
        });
    };
    if let Some(release) = config.release.as_deref() {
        resource_attr("service.version", release);
    }
    if let Some(environment) = config.environment.as_deref() {
        resource_attr("deployment.environment.name", environment);
    }
    resource_attr("telemetry.sdk.name", "erno-api");
    resource_attr("telemetry.sdk.version", env!("CARGO_PKG_VERSION"));

    opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest {
        resource_logs: vec![opentelemetry_proto::tonic::logs::v1::ResourceLogs {
            resource: Some(opentelemetry_proto::tonic::resource::v1::Resource {
                attributes: resource_attributes,
                ..Default::default()
            }),
            scope_logs: vec![opentelemetry_proto::tonic::logs::v1::ScopeLogs {
                log_records: reports.iter().map(to_log_record).collect(),
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

/// Drain the queue, batch, and deliver.
///
/// Mirrors the collector's writer: parks on `recv` when idle, so an application
/// that is not producing errors does no work at all.
pub async fn sender_loop(
    config: Arc<SenderConfig>,
    mut rx: mpsc::Receiver<CapturedError>,
    mut shutdown: crate::shutdown::Shutdown,
) {
    let client = match reqwest::Client::builder()
        .timeout(config.request_timeout)
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            // Never `tracing::error!` — the capture layer would feed it back in.
            eprintln!("error_reporting: could not build HTTP client: {e}");
            return;
        }
    };

    let mut breaker = CircuitBreaker::new(
        config.circuit_breaker_failures,
        config.circuit_breaker_cooldown,
    );

    loop {
        let first = tokio::select! {
            queued = rx.recv() => queued,
            () = shutdown.recv() => {
                // Drain whatever is already queued and send it as one last
                // batch. Anything arriving after this is lost, which is the
                // right trade: the process is going away either way, and
                // waiting indefinitely would hold the pod open past its grace
                // period.
                let mut remaining = Vec::new();
                while let Ok(error) = rx.try_recv() {
                    remaining.push(error);
                    if remaining.len() >= config.batch_size {
                        break;
                    }
                }
                if !remaining.is_empty() {
                    deliver(&client, &config, &mut breaker, remaining).await;
                }
                return;
            }
        };
        let Some(first) = first else {
            break;
        };

        let mut reports = vec![first];
        let deadline = tokio::time::sleep(config.flush_interval);
        tokio::pin!(deadline);
        let mut closed = false;

        loop {
            tokio::select! {
                () = &mut deadline => break,
                message = rx.recv() => match message {
                    Some(report) => {
                        reports.push(report);
                        if reports.len() >= config.batch_size {
                            break;
                        }
                    }
                    None => {
                        closed = true;
                        break;
                    }
                },
            }
        }

        deliver(&client, &config, &mut breaker, reports).await;

        if closed {
            break;
        }
    }
}

async fn deliver(
    client: &reqwest::Client,
    config: &SenderConfig,
    breaker: &mut CircuitBreaker,
    reports: Vec<CapturedError>,
) {
    let count = reports.len() as u64;

    if !breaker.allows(Instant::now()) {
        // The collector is known to be down; do not add to the pile.
        metrics::counter!("erno_error_reports_dropped_total", "reason" => "circuit_open")
            .increment(count);
        return;
    }

    let body = {
        use prost::Message as _;
        build_export_request(&reports, config).encode_to_vec()
    };

    for attempt in 1..=MAX_ATTEMPTS {
        let result = client
            .post(&config.endpoint)
            .bearer_auth(&config.token)
            .header("content-type", "application/x-protobuf")
            .body(body.clone())
            .send()
            .await;

        let (disposition, retry_after) = match &result {
            Ok(response) => (
                classify_status(response.status().as_u16()),
                retry_after_of(response),
            ),
            // A transport error — DNS, connect, timeout — is transient.
            Err(_) => (Disposition::Retry, None),
        };

        match disposition {
            Disposition::Delivered => {
                breaker.record_success();
                metrics::counter!("erno_error_reports_sent_total").increment(count);
                return;
            }
            Disposition::Discard => {
                // The collector will never accept this payload; retrying it
                // would be a permanent hot loop.
                breaker.record_success();
                metrics::counter!("erno_error_reports_dropped_total", "reason" => "rejected")
                    .increment(count);
                return;
            }
            Disposition::Retry => {
                breaker.record_failure(Instant::now());
                if attempt == MAX_ATTEMPTS || breaker.is_open() {
                    break;
                }
                let delay = retry_after.unwrap_or_else(|| backoff::next_delay(attempt));
                tokio::time::sleep(delay).await;
            }
        }
    }

    metrics::counter!("erno_error_reports_dropped_total", "reason" => "unreachable")
        .increment(count);
}

fn retry_after_of(response: &reqwest::Response) -> Option<Duration> {
    let seconds: u64 = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    // Do not let a hostile or broken collector park the sender for hours.
    Some(Duration::from_secs(seconds.min(300)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error_reporting::{Level, Source};

    #[test]
    fn success_statuses_are_delivered() {
        assert_eq!(classify_status(200), Disposition::Delivered);
        assert_eq!(classify_status(202), Disposition::Delivered);
        assert_eq!(classify_status(204), Disposition::Delivered);
    }

    #[test]
    fn a_rejected_payload_is_discarded_not_retried() {
        // The distinction that stops a bad payload becoming a permanent loop.
        assert_eq!(classify_status(400), Disposition::Discard);
        assert_eq!(classify_status(401), Disposition::Discard);
        assert_eq!(classify_status(413), Disposition::Discard);
        assert_eq!(classify_status(422), Disposition::Discard);
    }

    #[test]
    fn overload_and_collector_faults_are_retried() {
        assert_eq!(classify_status(429), Disposition::Retry);
        assert_eq!(classify_status(500), Disposition::Retry);
        assert_eq!(classify_status(502), Disposition::Retry);
        assert_eq!(classify_status(503), Disposition::Retry);
    }

    fn config() -> SenderConfig {
        SenderConfig {
            endpoint: "https://monitoring.test/api/otlp/v1/logs".to_string(),
            token: "t".to_string(),
            batch_size: 10,
            flush_interval: Duration::from_millis(10),
            request_timeout: Duration::from_secs(5),
            circuit_breaker_failures: 3,
            circuit_breaker_cooldown: Duration::from_secs(60),
            release: Some("1.2.3".to_string()),
            environment: Some("production".to_string()),
        }
    }

    fn attr<'a>(
        record: &'a opentelemetry_proto::tonic::logs::v1::LogRecord,
        key: &str,
    ) -> Option<&'a str> {
        use opentelemetry_proto::tonic::common::v1::any_value;
        record
            .attributes
            .iter()
            .find(|kv| kv.key == key)
            .and_then(|kv| match kv.value.as_ref()?.value.as_ref()? {
                any_value::Value::StringValue(s) => Some(s.as_str()),
                _ => None,
            })
    }

    #[test]
    fn a_report_becomes_a_semconv_log_record() {
        let mut report = CapturedError::new(
            Source::Api,
            Level::Fatal,
            "panic".to_string(),
            "attempt to divide by zero".to_string(),
        );
        report.user_id = Some(uuid::Uuid::nil());
        report.user_email = Some("a@example.com".to_string());

        let request = build_export_request(std::slice::from_ref(&report), &config());
        let record = &request.resource_logs[0].scope_logs[0].log_records[0];
        assert_eq!(record.severity_number, 21, "fatal is 21");
        assert_eq!(record.severity_text, "fatal");
        assert_eq!(attr(record, "exception.type"), Some("panic"));
        assert_eq!(attr(record, "erno.source"), Some("api"));
        assert_eq!(
            attr(record, "enduser.id"),
            Some("00000000-0000-0000-0000-000000000000")
        );
        assert_eq!(attr(record, "user.email"), Some("a@example.com"));
        // Absent optionals are absent, not empty strings.
        assert_eq!(attr(record, "exception.stacktrace"), None);
        assert_eq!(attr(record, "erno.frames"), None);
        assert!(record.time_unix_nano > 0);

        // Release and environment ride the resource, where the collector
        // reads them for every record at once.
        let resource = request.resource_logs[0].resource.as_ref().unwrap();
        let get = |key: &str| {
            resource
                .attributes
                .iter()
                .find(|kv| kv.key == key)
                .map(|kv| {
                    use opentelemetry_proto::tonic::common::v1::any_value;
                    match kv.value.as_ref().unwrap().value.as_ref().unwrap() {
                        any_value::Value::StringValue(s) => s.clone(),
                        other => format!("{other:?}"),
                    }
                })
        };
        assert_eq!(get("service.version").as_deref(), Some("1.2.3"));
        assert_eq!(
            get("deployment.environment.name").as_deref(),
            Some("production")
        );
        assert_eq!(get("telemetry.sdk.name").as_deref(), Some("erno-api"));
    }

    #[test]
    fn the_call_site_travels_under_the_semconv_key() {
        let mut report =
            CapturedError::new(Source::Api, Level::Error, "E".to_string(), "m".to_string());
        report.context = serde_json::json!({ "file": "src/sync.rs", "entity": "solves" });
        let request = build_export_request(std::slice::from_ref(&report), &config());
        let record = &request.resource_logs[0].scope_logs[0].log_records[0];
        assert_eq!(attr(record, "code.filepath"), Some("src/sync.rs"));
        assert_eq!(attr(record, "entity"), Some("solves"));
        assert_eq!(attr(record, "file"), None, "the raw key does not also ride");
    }

    #[test]
    fn frames_and_fingerprints_ride_as_lossless_json() {
        let mut report =
            CapturedError::new(Source::App, Level::Error, "E".to_string(), "m".to_string());
        report.frames = vec![crate::error_reporting::Frame {
            function: Some("doWork".to_string()),
            file: Some("main.js".to_string()),
            line: Some(10),
            column: None,
            in_app: true,
        }];
        report.client_fingerprint = Some(vec!["checkout".to_string()]);
        let request = build_export_request(std::slice::from_ref(&report), &config());
        let record = &request.resource_logs[0].scope_logs[0].log_records[0];
        let frames: Vec<crate::error_reporting::Frame> =
            serde_json::from_str(attr(record, "erno.frames").unwrap()).unwrap();
        assert_eq!(frames[0].file.as_deref(), Some("main.js"));
        let fp: Vec<String> =
            serde_json::from_str(attr(record, "erno.fingerprint").unwrap()).unwrap();
        assert_eq!(fp, vec!["checkout"]);
    }
}
