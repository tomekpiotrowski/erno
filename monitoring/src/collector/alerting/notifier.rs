//! Delivering alert notifications.
//!
//! Docs: docs/src/content/docs/monitoring/alerts.md

use lettre::{message::header::ContentType, Message};
use uuid::Uuid;

use super::rules::Notify;
use crate::collector::models::alert_rule;
use erno::mailer::{Mailer, MockEmailRecord};

/// What a notifier needs beyond the rule itself.
#[derive(Debug, Clone)]
pub struct NotifyContext {
    /// Envelope sender for emails.
    pub sender: String,
    /// Fallback recipient when a rule does not name one.
    pub default_recipient: Option<String>,
    /// Base URL of the monitoring console, for deep links.
    pub console_url: String,
}

/// Send one notification.
///
/// Failures are counted and dropped rather than retried: the rule is evaluated
/// again shortly, and a stale duplicate is worse than a gap.
pub async fn send(
    mailer: &Mailer,
    client: &reqwest::Client,
    context: &NotifyContext,
    rule: &alert_rule::Model,
    notify: Notify,
    description: &str,
) {
    let (verb, subject_prefix) = match notify {
        Notify::Firing => ("started", "FIRING"),
        Notify::Resolved => ("resolved", "RESOLVED"),
        Notify::Nothing => return,
    };

    let subject = format!("[erno] {subject_prefix}: {}", rule.name);
    let body_text = format!(
        "Alert {verb}: {}\n\n{description}\n\nSeverity: {}\nRule: {} {} {}\n\n{}/alerts",
        rule.name,
        rule.severity,
        rule.source,
        rule.comparator,
        rule.threshold,
        context.console_url.trim_end_matches('/'),
    );

    let recipient = rule
        .notify_email
        .clone()
        .or_else(|| context.default_recipient.clone());

    if let Some(recipient) = recipient.filter(|r| !r.trim().is_empty()) {
        send_email(mailer, context, &recipient, &subject, &body_text).await;
    }

    if let Some(url) = rule.notify_webhook.as_deref().filter(|u| !u.is_empty()) {
        send_webhook(client, url, rule, notify, description).await;
    }
}

async fn send_email(
    mailer: &Mailer,
    context: &NotifyContext,
    recipient: &str,
    subject: &str,
    body_text: &str,
) {
    let body_html = format!("<p>{}</p>", html_escape(body_text).replace('\n', "<br />"));

    // Keeps the dev inbox at /dev/emails working for alerts too.
    mailer.store_record(MockEmailRecord {
        id: Uuid::new_v4(),
        to: recipient.to_string(),
        from: context.sender.clone(),
        subject: subject.to_string(),
        body_html: Some(body_html.clone()),
        body_text: Some(body_text.to_string()),
        created_at: chrono::Utc::now(),
    });

    let (Ok(from), Ok(to)) = (context.sender.parse(), recipient.parse()) else {
        eprintln!("alerting: invalid sender or recipient address");
        return;
    };

    let message = Message::builder()
        .from(from)
        .to(to)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(body_html);

    let Ok(message) = message else {
        eprintln!("alerting: could not build the notification email");
        return;
    };

    let timer = erno::metrics::OperationTimer::start(
        "erno_email_send_duration_seconds",
        "erno_email_send_total",
        "template",
        "alert_notification",
    );
    let outcome = mailer.send(message).await;
    timer.finish(&outcome);

    match outcome {
        Ok(()) => {
            metrics::counter!("erno_alert_notifications_total", "channel" => "email", "result" => "sent")
                .increment(1);
        }
        Err(e) => {
            // Never `tracing::error!` — the capture layer would turn a failed
            // notification into an error report, which could then alert.
            eprintln!("alerting: could not send notification email: {e}");
            metrics::counter!("erno_alert_notifications_total", "channel" => "email", "result" => "failed")
                .increment(1);
        }
    }
}

async fn send_webhook(
    client: &reqwest::Client,
    url: &str,
    rule: &alert_rule::Model,
    notify: Notify,
    description: &str,
) {
    let payload = serde_json::json!({
        "status": match notify {
            Notify::Firing => "firing",
            Notify::Resolved => "resolved",
            Notify::Nothing => "nothing",
        },
        "rule": rule.name,
        "severity": rule.severity,
        "source": rule.source,
        "selector": rule.selector,
        "threshold": rule.threshold,
        "description": description,
    });

    match client.post(url).json(&payload).send().await {
        Ok(response) if response.status().is_success() => {
            metrics::counter!("erno_alert_notifications_total", "channel" => "webhook", "result" => "sent")
                .increment(1);
        }
        Ok(response) => {
            eprintln!("alerting: webhook returned {}", response.status());
            metrics::counter!("erno_alert_notifications_total", "channel" => "webhook", "result" => "rejected")
                .increment(1);
        }
        Err(e) => {
            eprintln!("alerting: could not reach webhook: {e}");
            metrics::counter!("erno_alert_notifications_total", "channel" => "webhook", "result" => "failed")
                .increment(1);
        }
    }
}

/// An alert description is arbitrary text and ends up in HTML.
fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_are_escaped_before_they_reach_html() {
        assert_eq!(
            html_escape("<script>alert('x')</script>"),
            "&lt;script&gt;alert('x')&lt;/script&gt;"
        );
    }
}
