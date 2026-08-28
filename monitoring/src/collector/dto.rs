//! Ingest envelope and its sanitizer.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! The governing rule: **oversized input is truncated, never rejected.** A
//! reporter that receives a 4xx retries forever, which turns one bad payload
//! into a permanent hot loop against the collector. Only JSON that will not
//! deserialize at all is refused.

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use erno::error_reporting::{CapturedError, Frame, Level, Source};

use crate::scrub;

/// Longest stored message.
pub const MAX_MESSAGE_LEN: usize = 4096;
/// Longest stored raw stack.
pub const MAX_STACK_LEN: usize = 16 * 1024;
/// Longest stored exception type.
pub const MAX_TYPE_LEN: usize = 200;
/// Most frames kept per event.
pub const MAX_FRAMES: usize = 50;
/// Largest serialized context blob.
pub const MAX_CONTEXT_BYTES: usize = 8 * 1024;
/// How far from now a client-supplied timestamp may be before it is clamped.
pub const MAX_CLOCK_SKEW_MINUTES: i64 = 5;

/// One ingest request.
#[derive(Debug, Clone, Deserialize)]
pub struct IngestEnvelope {
    /// The reports. An empty list is accepted and does nothing.
    #[serde(default)]
    pub events: Vec<IngestEvent>,
    /// Release applied to every event that does not carry its own.
    #[serde(default)]
    pub release: Option<String>,
    /// Environment applied to every event that does not carry its own.
    #[serde(default)]
    pub environment: Option<String>,
    /// Reporting SDK, recorded in context for support purposes.
    #[serde(default)]
    pub sdk: Option<SdkInfo>,
}

/// Identifies the reporting client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkInfo {
    /// e.g. `erno-angular`.
    pub name: String,
    /// SDK version.
    #[serde(default)]
    pub version: Option<String>,
}

/// A single reported error.
#[derive(Debug, Clone, Deserialize)]
pub struct IngestEvent {
    /// Exception type or error class. Required.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Error message. Required.
    pub message: String,
    /// `warning` | `error` | `fatal`. Anything else becomes `error`.
    #[serde(default)]
    pub level: Option<String>,
    /// When it happened. Clamped to now ± [`MAX_CLOCK_SKEW_MINUTES`], because a
    /// client with a wrong clock would otherwise poison `first_seen` forever.
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    /// Raw stack text.
    #[serde(default)]
    pub stack: Option<String>,
    /// Parsed frames.
    #[serde(default)]
    pub frames: Option<Vec<Frame>>,
    /// Explicit grouping override.
    #[serde(default)]
    pub fingerprint: Option<Vec<String>>,
    /// Structured context.
    #[serde(default)]
    pub context: Option<Value>,
    /// Affected user. Honoured **only** on the trusted server-to-server path —
    /// otherwise any anonymous caller could attribute errors to anyone.
    #[serde(default)]
    pub user: Option<IngestUser>,
}

/// User attribution supplied by a trusted reporter.
#[derive(Debug, Clone, Deserialize)]
pub struct IngestUser {
    /// Application user id.
    #[serde(default)]
    pub id: Option<Uuid>,
    /// Application user email.
    #[serde(default)]
    pub email: Option<String>,
}

/// What ingest always answers with, including when reports were shed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestResponse {
    /// Reports queued or written.
    pub accepted: usize,
    /// Reports discarded — over the per-request cap, or the queue was full.
    pub dropped: usize,
}

/// Everything the collector needs to know about who is calling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestOrigin {
    /// Source assigned from the credential.
    pub source: Source,
    /// Whether user attribution on the wire may be believed.
    pub trusted: bool,
}

/// Convert a wire event into the internal form, truncating anything oversized
/// and scrubbing secrets before they are ever persisted.
///
/// `store_client_ip` gates IP retention; when false the address is dropped
/// here rather than being written and cleaned up later.
#[must_use]
pub fn sanitize(
    event: IngestEvent,
    envelope_release: Option<&str>,
    envelope_environment: Option<&str>,
    sdk: Option<&SdkInfo>,
    origin: IngestOrigin,
    client_ip: Option<&str>,
    store_client_ip: bool,
) -> CapturedError {
    let level = event
        .level
        .as_deref()
        .map_or(Level::Error, Level::from_str_or_error);

    let error_type = truncate(event.error_type.trim(), MAX_TYPE_LEN);
    let error_type = if error_type.is_empty() {
        "Error".to_string()
    } else {
        error_type
    };

    // Scrub first, then truncate. Truncating first could split a secret across
    // the cut so the pattern no longer matches, leaving half of it stored.
    let message = truncate(&scrub::scrub_text(event.message.trim()), MAX_MESSAGE_LEN);
    let stack = event
        .stack
        .map(|s| truncate(&scrub::scrub_text(&s), MAX_STACK_LEN));

    let frames = normalize_frames(event.frames.unwrap_or_default());
    let context = build_context(event.context, sdk, client_ip, store_client_ip);

    // Only the trusted server-to-server path may attribute a user.
    let (user_id, user_email) = match (origin.trusted, event.user) {
        (true, Some(user)) => (user.id, user.email.map(|e| truncate(&e, 320))),
        _ => (None, None),
    };

    CapturedError {
        source: origin.source,
        level,
        error_type,
        message,
        stack,
        frames,
        context,
        release: envelope_release.map(|r| truncate(r, 200)),
        environment: envelope_environment.map(|e| truncate(e, 100)),
        user_id,
        user_email,
        client_ip: if store_client_ip {
            client_ip.map(|ip| truncate(ip, 64))
        } else {
            None
        },
        client_fingerprint: event.fingerprint,
        timestamp: clamp_timestamp(event.timestamp),
    }
}

/// Cap frame count and recompute `in_app` rather than trusting the wire.
fn normalize_frames(mut frames: Vec<Frame>) -> Vec<Frame> {
    frames.truncate(MAX_FRAMES);
    for frame in &mut frames {
        frame.in_app = erno::error_reporting::is_in_app(frame.file.as_deref());
        if let Some(file) = &mut frame.file {
            *file = scrub::scrub_url(file);
        }
    }
    frames
}

/// Assemble the context object, scrub it, and bound its serialized size.
fn build_context(
    context: Option<Value>,
    sdk: Option<&SdkInfo>,
    client_ip: Option<&str>,
    store_client_ip: bool,
) -> Value {
    let mut map = match context {
        Some(Value::Object(map)) => map,
        // A non-object context is kept rather than dropped — it is still a clue.
        Some(other) => {
            let mut map = Map::new();
            map.insert("value".to_string(), other);
            map
        }
        None => Map::new(),
    };

    if let Some(sdk) = sdk {
        if let Ok(value) = serde_json::to_value(sdk) {
            map.insert("sdk".to_string(), value);
        }
    }
    if store_client_ip {
        if let Some(ip) = client_ip {
            map.insert("client_ip".to_string(), Value::String(ip.to_string()));
        }
    }

    let mut value = Value::Object(map);
    scrub::scrub_value(&mut value);
    bound_context_size(value)
}

/// Replace an over-large context with a marker rather than storing it.
fn bound_context_size(value: Value) -> Value {
    let too_big = serde_json::to_vec(&value).is_ok_and(|v| v.len() > MAX_CONTEXT_BYTES);
    if !too_big {
        return value;
    }

    let mut map = Map::new();
    map.insert(
        "truncated".to_string(),
        Value::String(format!(
            "context exceeded {MAX_CONTEXT_BYTES} bytes and was dropped"
        )),
    );
    // Keep the small, high-value keys if they are present.
    if let Value::Object(original) = value {
        for key in ["url", "route", "method", "status", "user_agent"] {
            if let Some(v) = original.get(key) {
                if serde_json::to_vec(v).is_ok_and(|s| s.len() < 512) {
                    map.insert(key.to_string(), v.clone());
                }
            }
        }
    }
    Value::Object(map)
}

/// Clamp a client timestamp into a believable window around now.
fn clamp_timestamp(timestamp: Option<DateTime<Utc>>) -> NaiveDateTime {
    let now = Utc::now();
    let Some(ts) = timestamp else {
        return now.naive_utc();
    };
    let skew = Duration::minutes(MAX_CLOCK_SKEW_MINUTES);
    let clamped = ts.clamp(now - skew, now + skew);
    clamped.naive_utc()
}

/// Truncate on a character boundary — input is arbitrary UTF-8.
fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn origin(trusted: bool) -> IngestOrigin {
        IngestOrigin {
            source: if trusted { Source::Api } else { Source::App },
            trusted,
        }
    }

    fn event(error_type: &str, message: &str) -> IngestEvent {
        IngestEvent {
            error_type: error_type.to_string(),
            message: message.to_string(),
            level: None,
            timestamp: None,
            stack: None,
            frames: None,
            fingerprint: None,
            context: None,
            user: None,
        }
    }

    fn sanitize_default(e: IngestEvent, trusted: bool) -> CapturedError {
        sanitize(e, None, None, None, origin(trusted), None, false)
    }

    #[test]
    fn oversized_fields_are_truncated_not_rejected() {
        let mut e = event("T", &"failed to load record ".repeat(MAX_MESSAGE_LEN));
        e.stack = Some("    at Foo (main.js)\n".repeat(MAX_STACK_LEN));
        e.error_type = "Type ".repeat(MAX_TYPE_LEN);
        let out = sanitize_default(e, false);
        assert_eq!(out.message.chars().count(), MAX_MESSAGE_LEN);
        assert_eq!(out.stack.unwrap().chars().count(), MAX_STACK_LEN);
        assert_eq!(out.error_type.chars().count(), MAX_TYPE_LEN);
    }

    #[test]
    fn frames_are_capped_and_in_app_is_recomputed() {
        let mut e = event("T", "m");
        e.frames = Some(
            (0..100)
                .map(|i| Frame {
                    function: Some(format!("f{i}")),
                    // Claims to be first-party, but the path says otherwise.
                    file: Some("/app/node_modules/x/y.js".to_string()),
                    line: Some(1),
                    column: None,
                    in_app: true,
                })
                .collect(),
        );
        let out = sanitize_default(e, false);
        assert_eq!(out.frames.len(), MAX_FRAMES);
        assert!(
            out.frames.iter().all(|f| !f.in_app),
            "wire in_app must not be trusted"
        );
    }

    #[test]
    fn an_empty_type_falls_back_rather_than_failing() {
        let out = sanitize_default(event("   ", "m"), false);
        assert_eq!(out.error_type, "Error");
    }

    #[test]
    fn untrusted_callers_cannot_attribute_a_user() {
        let mut e = event("T", "m");
        e.user = Some(IngestUser {
            id: Some(Uuid::new_v4()),
            email: Some("victim@example.com".to_string()),
        });
        let out = sanitize_default(e, false);
        assert!(out.user_id.is_none());
        assert!(out.user_email.is_none());
    }

    #[test]
    fn trusted_callers_may_attribute_a_user() {
        let id = Uuid::new_v4();
        let mut e = event("T", "m");
        e.user = Some(IngestUser {
            id: Some(id),
            email: Some("real@example.com".to_string()),
        });
        let out = sanitize_default(e, true);
        assert_eq!(out.user_id, Some(id));
        assert_eq!(out.user_email.as_deref(), Some("real@example.com"));
    }

    #[test]
    fn secrets_are_scrubbed_before_they_are_ever_stored() {
        let mut e = event("T", "failed with Bearer abcdef1234567890");
        e.context = Some(json!({ "headers": { "authorization": "Bearer xyz123456789" } }));
        let out = sanitize_default(e, false);
        assert!(!out.message.contains("abcdef1234567890"), "{}", out.message);
        assert_eq!(out.context["headers"]["authorization"], scrub::REDACTED);
    }

    #[test]
    fn frame_files_have_their_query_and_fragment_scrubbed() {
        let mut e = event("T", "m");
        e.frames = Some(vec![Frame {
            function: Some("f".to_string()),
            file: Some("https://app.test/main.js?token=secretvalue#frag".to_string()),
            line: Some(1),
            column: None,
            in_app: false,
        }]);
        let out = sanitize_default(e, false);
        let file = out.frames[0].file.as_deref().unwrap();
        assert!(!file.contains("secretvalue"), "{file}");
        assert!(!file.contains("frag"), "{file}");
    }

    #[test]
    fn future_and_ancient_timestamps_are_clamped() {
        let mut e = event("T", "m");
        e.timestamp = Some(Utc::now() + Duration::days(3650));
        let far_future = sanitize_default(e, false).timestamp;
        assert!(far_future <= (Utc::now() + Duration::minutes(6)).naive_utc());

        let mut e = event("T", "m");
        e.timestamp = Some(Utc::now() - Duration::days(3650));
        let far_past = sanitize_default(e, false).timestamp;
        assert!(far_past >= (Utc::now() - Duration::minutes(6)).naive_utc());
    }

    #[test]
    fn a_reasonable_timestamp_is_preserved() {
        let when = Utc::now() - Duration::minutes(1);
        let mut e = event("T", "m");
        e.timestamp = Some(when);
        let out = sanitize_default(e, false);
        assert_eq!(out.timestamp, when.naive_utc());
    }

    #[test]
    fn client_ip_is_dropped_unless_configured() {
        let out = sanitize(
            event("T", "m"),
            None,
            None,
            None,
            origin(false),
            Some("203.0.113.9"),
            false,
        );
        assert!(out.client_ip.is_none());
        assert!(out.context.get("client_ip").is_none());

        let out = sanitize(
            event("T", "m"),
            None,
            None,
            None,
            origin(false),
            Some("203.0.113.9"),
            true,
        );
        assert_eq!(out.client_ip.as_deref(), Some("203.0.113.9"));
    }

    #[test]
    fn an_oversized_context_is_replaced_but_keeps_the_useful_keys() {
        let mut e = event("T", "m");
        e.context = Some(json!({
            "url": "https://app.test/x",
            "route": "/x",
            // Many short distinct strings: too big to keep, but nothing the
            // scrubber would collapse into a single redaction.
            "blob": (0..MAX_CONTEXT_BYTES / 4).map(|i| format!("k{i}")).collect::<Vec<_>>(),
        }));
        let out = sanitize_default(e, false);
        assert!(out.context.get("truncated").is_some());
        assert_eq!(out.context["url"], "https://app.test/x");
        assert_eq!(out.context["route"], "/x");
        assert!(out.context.get("blob").is_none());
    }

    #[test]
    fn a_non_object_context_is_kept_under_a_value_key() {
        let mut e = event("T", "m");
        e.context = Some(json!("just a string"));
        let out = sanitize_default(e, false);
        assert_eq!(out.context["value"], "just a string");
    }

    #[test]
    fn sdk_info_lands_in_context() {
        let sdk = SdkInfo {
            name: "erno-angular".to_string(),
            version: Some("0.1.0".to_string()),
        };
        let out = sanitize(
            event("T", "m"),
            Some("1.2.3"),
            Some("production"),
            Some(&sdk),
            origin(false),
            None,
            false,
        );
        assert_eq!(out.context["sdk"]["name"], "erno-angular");
        assert_eq!(out.release.as_deref(), Some("1.2.3"));
        assert_eq!(out.environment.as_deref(), Some("production"));
    }

    #[test]
    fn envelope_deserializes_from_the_documented_shape() {
        let raw = r#"{
            "events": [{
                "type": "TypeError",
                "message": "x is not a function",
                "level": "error",
                "stack": "TypeError: x\n    at Foo (main.js:1:2)",
                "frames": [{"function":"Foo","file":"main.js","line":1,"column":2}],
                "context": {"url":"https://app.test/x"}
            }],
            "release": "1.0.0",
            "environment": "production",
            "sdk": {"name":"erno-angular","version":"0.1.0"}
        }"#;
        let envelope: IngestEnvelope = serde_json::from_str(raw).expect("valid envelope");
        assert_eq!(envelope.events.len(), 1);
        assert_eq!(envelope.events[0].error_type, "TypeError");
        assert_eq!(envelope.release.as_deref(), Some("1.0.0"));
        assert_eq!(envelope.events[0].frames.as_ref().unwrap()[0].line, Some(1));
    }

    #[test]
    fn unknown_fields_are_ignored_so_sdks_can_move_ahead_of_collectors() {
        let raw = r#"{"events":[{"type":"E","message":"m","brand_new_field":true}]}"#;
        let envelope: IngestEnvelope = serde_json::from_str(raw).expect("valid envelope");
        assert_eq!(envelope.events.len(), 1);
    }
}
