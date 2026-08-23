//! New-issue email alerting.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Runs in its own task, fed by the writer. Two reasons it is not inline:
//! SMTP is slow and must never stall the write loop, and a mail failure must
//! never fail a batch that has already been persisted.
//!
//! Alerting is sent **directly** rather than through the job queue. Registering
//! a built-in job type is a breaking config change — `verify_job_types_have_workers`
//! panics at boot if a registered type is missing from a worker pool, so every
//! existing deployment's TOML would need editing before it would start again.
//! That is far too high a price for an email. The tradeoff is no SMTP retry: a
//! failed alert is counted and dropped.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sea_orm::DatabaseConnection;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    email_templates::{render_or_fallback, EmailTemplate},
    error_reporting::{config::AlertsConfig, Level},
    mailer::{Mailer, MockEmailRecord},
};

use super::ingest::{mark_alerted, NewIssue};

/// What the alert task needs that it cannot derive itself.
#[derive(Debug, Clone)]
pub struct AlertContext {
    /// Operator recipient and throttle settings.
    pub config: AlertsConfig,
    /// Envelope sender.
    pub sender: String,
    /// Base URL of the monitoring console, for deep links.
    pub console_url: String,
    /// Optional directory of branded templates.
    pub templates_dir: Option<String>,
}

/// Decides whether an alert may go out right now.
///
/// Time is passed in rather than read, so the whole policy is testable without
/// sleeping.
#[derive(Debug)]
pub struct AlertThrottle {
    max_per_window: usize,
    window: Duration,
    min_interval: Duration,
    window_started: Option<Instant>,
    sent_in_window: usize,
    suppressed: usize,
    last_sent: Option<Instant>,
}

/// What the throttle decided about one candidate alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertDecision {
    /// Send it.
    Send,
    /// Hold it back; it counts toward the digest.
    Suppress,
    /// Too soon after the last one.
    TooSoon,
}

impl AlertThrottle {
    /// Build a throttle from configuration.
    #[must_use]
    pub fn new(config: &AlertsConfig) -> Self {
        Self {
            max_per_window: config.max_per_window,
            window: Duration::from_secs(config.window_minutes.saturating_mul(60).max(1)),
            min_interval: Duration::from_secs(config.min_interval_seconds),
            window_started: None,
            sent_in_window: 0,
            suppressed: 0,
            last_sent: None,
        }
    }

    /// Roll the window if it has elapsed, returning any suppressed count that
    /// now needs a digest.
    pub fn roll_window(&mut self, now: Instant) -> Option<usize> {
        let started = match self.window_started {
            Some(started) => started,
            None => {
                self.window_started = Some(now);
                return None;
            }
        };
        if now.duration_since(started) < self.window {
            return None;
        }

        let suppressed = self.suppressed;
        self.window_started = Some(now);
        self.sent_in_window = 0;
        self.suppressed = 0;
        (suppressed > 0).then_some(suppressed)
    }

    /// Decide on one candidate.
    pub fn decide(&mut self, now: Instant) -> AlertDecision {
        if self.window_started.is_none() {
            self.window_started = Some(now);
        }
        if self.sent_in_window >= self.max_per_window {
            self.suppressed += 1;
            return AlertDecision::Suppress;
        }
        if let Some(last) = self.last_sent {
            if now.duration_since(last) < self.min_interval {
                return AlertDecision::TooSoon;
            }
        }
        self.sent_in_window += 1;
        self.last_sent = Some(now);
        AlertDecision::Send
    }

    /// Issues held back in the current window.
    #[must_use]
    pub const fn suppressed(&self) -> usize {
        self.suppressed
    }
}

/// Drain new issues and mail the operator about them.
pub async fn alert_loop(
    db: DatabaseConnection,
    mailer: Mailer,
    context: AlertContext,
    mut rx: mpsc::Receiver<Vec<NewIssue>>,
) {
    let mut throttle = AlertThrottle::new(&context.config);
    let minimum = context.config.minimum_level();

    while let Some(issues) = rx.recv().await {
        let now = Instant::now();

        if let Some(suppressed) = throttle.roll_window(now) {
            send_digest(&mailer, &context, suppressed).await;
        }

        let mut alerted: Vec<Uuid> = Vec::new();
        for issue in issues {
            if issue.level.rank() < minimum.rank() {
                continue;
            }
            match throttle.decide(Instant::now()) {
                AlertDecision::Send => {
                    if send_alert(&mailer, &context, &issue).await {
                        alerted.push(issue.id);
                    }
                }
                // Both mean "not now". `alert_sent_at` stays unset, so nothing
                // claims an alert went out that did not.
                AlertDecision::Suppress | AlertDecision::TooSoon => {}
            }
        }

        if !alerted.is_empty() {
            if let Err(e) = mark_alerted(&db, &alerted).await {
                // Never `tracing::error!`: the capture layer would feed this back.
                eprintln!("error_reporting: could not mark issues alerted: {e}");
            }
        }
    }
}

async fn send_alert(mailer: &Mailer, context: &AlertContext, issue: &NewIssue) -> bool {
    let subject = format!(
        "[erno] New error: {} — {}",
        issue.error_type,
        truncate(&issue.title, 60)
    );

    let issue_url = format!(
        "{}/issues/{}",
        context.console_url.trim_end_matches('/'),
        issue.id
    );

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("error_type", issue.error_type.clone());
    vars.insert("title", issue.title.clone());
    vars.insert("culprit", issue.culprit.clone().unwrap_or_default());
    vars.insert("source", issue.source.to_string());
    vars.insert("level", issue.level.to_string());
    vars.insert("release", issue.release.clone().unwrap_or_default());
    vars.insert("environment", issue.environment.clone().unwrap_or_default());
    vars.insert("issue_url", issue_url.clone());

    let body = render_or_fallback(
        context.templates_dir.as_deref(),
        EmailTemplate::NewIssue,
        &vars,
        fallback_body(issue, &issue_url),
    );

    deliver(mailer, context, &subject, body).await
}

async fn send_digest(mailer: &Mailer, context: &AlertContext, suppressed: usize) {
    let subject = format!("[erno] {suppressed} further new error types were suppressed");
    let body = format!(
        "<p>{suppressed} additional new error types appeared in the last window \
         and were not mailed individually.</p>\
         <p><a href=\"{url}\">Open the monitoring console</a></p>",
        url = html_escape(context.console_url.trim_end_matches('/')),
    );
    deliver(mailer, context, &subject, body).await;
}

async fn deliver(mailer: &Mailer, context: &AlertContext, subject: &str, body: String) -> bool {
    use lettre::{message::header::ContentType, Message};

    // Keeps the dev inbox at /dev/emails working for alerts too.
    mailer.store_record(MockEmailRecord {
        id: Uuid::new_v4(),
        to: context.config.recipient.clone(),
        from: context.sender.clone(),
        subject: subject.to_string(),
        body_html: Some(body.clone()),
        body_text: None,
        created_at: chrono::Utc::now(),
    });

    let message = Message::builder()
        .from(match context.sender.parse() {
            Ok(address) => address,
            Err(e) => {
                eprintln!("error_reporting: invalid alert sender address: {e}");
                return false;
            }
        })
        .to(match context.config.recipient.parse() {
            Ok(address) => address,
            Err(e) => {
                eprintln!("error_reporting: invalid alert recipient address: {e}");
                return false;
            }
        })
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(body);

    let message = match message {
        Ok(message) => message,
        Err(e) => {
            eprintln!("error_reporting: could not build alert email: {e}");
            return false;
        }
    };

    // Also timed through the shared helper, so every outbound email shows up in
    // one place regardless of which path sent it — this one cannot use
    // `emails::send_html_email_with_meta`, which needs an `App`.
    let timer = crate::metrics::OperationTimer::start(
        "erno_email_send_duration_seconds",
        "erno_email_send_total",
        "template",
        "new_issue_alert",
    );
    let outcome = mailer.send(message).await;
    timer.finish(&outcome);

    match outcome {
        Ok(()) => {
            metrics::counter!("erno_error_alert_emails_total", "result" => "sent").increment(1);
            true
        }
        Err(e) => {
            // No retry: see the module comment on why this does not go through
            // the job queue.
            eprintln!("error_reporting: alert email failed: {e}");
            metrics::counter!("erno_error_alert_emails_total", "result" => "failed").increment(1);
            false
        }
    }
}

fn fallback_body(issue: &NewIssue, issue_url: &str) -> String {
    let culprit = issue
        .culprit
        .as_deref()
        .map(|c| format!("<p><strong>Culprit:</strong> {}</p>", html_escape(c)))
        .unwrap_or_default();
    let release = issue
        .release
        .as_deref()
        .map(|r| format!("<p><strong>Release:</strong> {}</p>", html_escape(r)))
        .unwrap_or_default();

    format!(
        "<p>A new error type was seen for the first time.</p>\
         <p><strong>{error_type}</strong></p>\
         <p>{title}</p>\
         {culprit}{release}\
         <p><strong>Source:</strong> {source} · <strong>Level:</strong> {level}</p>\
         <p><a href=\"{issue_url}\">Open in the monitoring console</a></p>",
        error_type = html_escape(&issue.error_type),
        title = html_escape(&issue.title),
        source = issue.source,
        level = issue.level,
        issue_url = html_escape(issue_url),
    )
}

/// Minimal escaping — an error message is arbitrary text and ends up in HTML.
fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}

/// Level ordering helper used by the loop, exposed for tests.
#[must_use]
pub fn meets_minimum(level: Level, minimum: Level) -> bool {
    level.rank() >= minimum.rank()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(max_per_window: usize, window_minutes: u64, min_interval: u64) -> AlertsConfig {
        AlertsConfig {
            enabled: true,
            recipient: "ops@example.com".to_string(),
            min_level: "error".to_string(),
            max_per_window,
            window_minutes,
            min_interval_seconds: min_interval,
        }
    }

    #[test]
    fn alerts_flow_until_the_window_cap_then_are_suppressed() {
        let mut throttle = AlertThrottle::new(&config(3, 60, 0));
        let now = Instant::now();

        for _ in 0..3 {
            assert_eq!(throttle.decide(now), AlertDecision::Send);
        }
        // A bad deploy minting hundreds of fingerprints must not mail hundreds
        // of times.
        for _ in 0..100 {
            assert_eq!(throttle.decide(now), AlertDecision::Suppress);
        }
        assert_eq!(throttle.suppressed(), 100);
    }

    #[test]
    fn the_window_resets_and_reports_what_it_held_back() {
        let mut throttle = AlertThrottle::new(&config(1, 60, 0));
        let start = Instant::now();

        assert_eq!(throttle.decide(start), AlertDecision::Send);
        assert_eq!(throttle.decide(start), AlertDecision::Suppress);

        // Still inside the window.
        assert_eq!(
            throttle.roll_window(start + Duration::from_secs(59 * 60)),
            None
        );

        // Window elapsed: one digest for everything held back.
        assert_eq!(
            throttle.roll_window(start + Duration::from_secs(60 * 60)),
            Some(1)
        );
        // And the budget is available again.
        assert_eq!(
            throttle.decide(start + Duration::from_secs(60 * 60)),
            AlertDecision::Send
        );
    }

    #[test]
    fn a_quiet_window_produces_no_digest() {
        let mut throttle = AlertThrottle::new(&config(5, 60, 0));
        let start = Instant::now();
        throttle.roll_window(start);
        assert_eq!(
            throttle.roll_window(start + Duration::from_secs(3600)),
            None
        );
    }

    #[test]
    fn the_minimum_interval_paces_even_an_allowed_burst() {
        let mut throttle = AlertThrottle::new(&config(10, 60, 30));
        let start = Instant::now();

        assert_eq!(throttle.decide(start), AlertDecision::Send);
        assert_eq!(
            throttle.decide(start + Duration::from_secs(5)),
            AlertDecision::TooSoon
        );
        assert_eq!(
            throttle.decide(start + Duration::from_secs(30)),
            AlertDecision::Send
        );
    }

    #[test]
    fn worst_case_is_the_cap_plus_one_digest_per_window() {
        let mut throttle = AlertThrottle::new(&config(10, 60, 0));
        let start = Instant::now();

        let mut sent = 0;
        for _ in 0..10_000 {
            if throttle.decide(start) == AlertDecision::Send {
                sent += 1;
            }
        }
        let digest = throttle.roll_window(start + Duration::from_secs(3600));

        assert_eq!(sent, 10);
        assert_eq!(digest, Some(9_990));
    }

    #[test]
    fn severity_filtering() {
        assert!(meets_minimum(Level::Error, Level::Error));
        assert!(meets_minimum(Level::Fatal, Level::Error));
        assert!(!meets_minimum(Level::Warning, Level::Error));
        assert!(meets_minimum(Level::Warning, Level::Warning));
    }

    #[test]
    fn error_text_is_escaped_before_it_reaches_html() {
        let issue = NewIssue {
            id: Uuid::nil(),
            fingerprint: "f".to_string(),
            level: Level::Error,
            source: crate::error_reporting::Source::App,
            error_type: "TypeError".to_string(),
            title: "<script>alert('xss')</script>".to_string(),
            culprit: None,
            release: None,
            environment: None,
        };
        let body = fallback_body(&issue, "https://monitoring.test/issues/x");
        assert!(!body.contains("<script>"), "{body}");
        assert!(body.contains("&lt;script&gt;"));
    }
}
