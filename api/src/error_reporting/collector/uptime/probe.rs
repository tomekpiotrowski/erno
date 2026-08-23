//! Running a single synthetic probe.
//!
//! Docs: docs/src/content/docs/monitoring/uptime.md

use std::time::{Duration, Instant};

use super::state::ProbeOutcome;
use crate::error_reporting::collector::models::uptime_check;

/// Longest body read when a content assertion is configured.
///
/// Bounded because the probe must not be turned into a memory sink by whatever
/// it is pointed at.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// Probe one endpoint and report what happened.
///
/// Never returns an error: a probe that could not run *is* the result.
pub async fn run(client: &reqwest::Client, check: &uptime_check::Model) -> ProbeOutcome {
    let method =
        reqwest::Method::from_bytes(check.method.as_bytes()).unwrap_or(reqwest::Method::GET);
    let timeout = Duration::from_millis(u64::try_from(check.timeout_ms).unwrap_or(10_000).max(1));

    let started = Instant::now();
    let response = client
        .request(method, &check.url)
        .timeout(timeout)
        .send()
        .await;

    let elapsed =
        |started: Instant| u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);

    let response = match response {
        Ok(response) => response,
        Err(e) => {
            let reason = if e.is_timeout() {
                format!("timed out after {}ms", check.timeout_ms)
            } else if e.is_connect() {
                "could not connect".to_string()
            } else {
                truncate(&e.to_string(), 400)
            };
            return ProbeOutcome::failure(None, elapsed(started), reason);
        }
    };

    let status = response.status().as_u16();
    let expected = u16::try_from(check.expected_status).unwrap_or(200);
    if status != expected {
        return ProbeOutcome::failure(
            Some(status),
            elapsed(started),
            format!("expected status {expected}, got {status}"),
        );
    }

    // A server can return 200 while serving an error page, so an optional
    // content assertion is the difference between "responded" and "working".
    if let Some(needle) = check
        .assert_body_contains
        .as_deref()
        .filter(|n| !n.is_empty())
    {
        let body = match response.text().await {
            Ok(body) => body,
            Err(e) => {
                return ProbeOutcome::failure(
                    Some(status),
                    elapsed(started),
                    format!("could not read body: {}", truncate(&e.to_string(), 200)),
                );
            }
        };
        let body = &body[..body.len().min(MAX_BODY_BYTES)];
        if !body.contains(needle) {
            return ProbeOutcome::failure(
                Some(status),
                elapsed(started),
                format!("body did not contain {needle:?}"),
            );
        }
    }

    ProbeOutcome::success(status, elapsed(started))
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}
