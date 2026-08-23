//! Outbound delivery to a collector.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::mpsc;

use crate::error_reporting::{CapturedError, Frame};

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

/// The wire form of one report.
#[derive(Debug, Serialize)]
struct OutboundEvent<'a> {
    #[serde(rename = "type")]
    error_type: &'a str,
    message: &'a str,
    level: &'a str,
    timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stack: Option<&'a str>,
    #[serde(skip_serializing_if = "<[Frame]>::is_empty")]
    frames: &'a [Frame],
    #[serde(skip_serializing_if = "Option::is_none")]
    fingerprint: Option<&'a [String]>,
    context: &'a serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<OutboundUser<'a>>,
}

#[derive(Debug, Serialize)]
struct OutboundUser<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct OutboundEnvelope<'a> {
    events: Vec<OutboundEvent<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    environment: Option<&'a str>,
    sdk: OutboundSdk,
}

#[derive(Debug, Serialize)]
struct OutboundSdk {
    name: &'static str,
    version: &'static str,
}

fn build_envelope<'a>(
    reports: &'a [CapturedError],
    config: &'a SenderConfig,
) -> OutboundEnvelope<'a> {
    OutboundEnvelope {
        events: reports
            .iter()
            .map(|report| OutboundEvent {
                error_type: &report.error_type,
                message: &report.message,
                level: report.level.as_str(),
                timestamp: report.timestamp.and_utc().to_rfc3339(),
                stack: report.stack.as_deref(),
                frames: &report.frames,
                fingerprint: report.client_fingerprint.as_deref(),
                context: &report.context,
                user: (report.user_id.is_some() || report.user_email.is_some()).then_some(
                    OutboundUser {
                        id: report.user_id,
                        email: report.user_email.as_deref(),
                    },
                ),
            })
            .collect(),
        release: config.release.as_deref(),
        environment: config.environment.as_deref(),
        sdk: OutboundSdk {
            name: "erno-api",
            version: env!("CARGO_PKG_VERSION"),
        },
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

    let envelope = build_envelope(&reports, config);

    for attempt in 1..=MAX_ATTEMPTS {
        let result = client
            .post(&config.endpoint)
            .header(
                super::super::collector::auth::INGEST_KEY_HEADER,
                &config.token,
            )
            .json(&envelope)
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
            endpoint: "https://monitoring.test/api/errors".to_string(),
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

    #[test]
    fn the_envelope_matches_the_documented_wire_shape() {
        let mut report = CapturedError::new(
            Source::Api,
            Level::Fatal,
            "panic".to_string(),
            "attempt to divide by zero".to_string(),
        );
        report.user_id = Some(uuid::Uuid::nil());
        report.user_email = Some("a@example.com".to_string());

        let config = config();
        let envelope = build_envelope(std::slice::from_ref(&report), &config);
        let json = serde_json::to_value(&envelope).expect("serialises");

        assert_eq!(json["events"][0]["type"], "panic");
        assert_eq!(json["events"][0]["message"], "attempt to divide by zero");
        assert_eq!(json["events"][0]["level"], "fatal");
        assert_eq!(json["events"][0]["user"]["email"], "a@example.com");
        assert_eq!(json["release"], "1.2.3");
        assert_eq!(json["environment"], "production");
        assert_eq!(json["sdk"]["name"], "erno-api");
        // Absent optional fields are omitted rather than sent as null.
        assert!(json["events"][0].get("stack").is_none());
        assert!(json["events"][0].get("frames").is_none());
    }

    #[test]
    fn a_report_without_a_user_omits_the_user_object() {
        let report = CapturedError::new(
            Source::Api,
            Level::Error,
            "DbErr".to_string(),
            "boom".to_string(),
        );
        let config = config();
        let envelope = build_envelope(std::slice::from_ref(&report), &config);
        let json = serde_json::to_value(&envelope).expect("serialises");
        assert!(json["events"][0].get("user").is_none());
    }

    #[test]
    fn timestamps_go_out_as_rfc3339_utc() {
        let report =
            CapturedError::new(Source::Api, Level::Error, "E".to_string(), "m".to_string());
        let config = config();
        let envelope = build_envelope(std::slice::from_ref(&report), &config);
        let json = serde_json::to_value(&envelope).expect("serialises");
        let ts = json["events"][0]["timestamp"].as_str().unwrap();
        assert!(ts.ends_with("+00:00"), "expected UTC offset, got {ts}");
        chrono::DateTime::parse_from_rfc3339(ts).expect("round-trips");
    }
}
