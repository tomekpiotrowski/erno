//! The handle every Erno application holds.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::{
    app_info::AppInfo,
    environment::Environment,
    error_reporting::{config::ErrorReportingConfig, CapturedError},
};

use super::sender::{sender_loop, SenderConfig};

/// A cheap, cloneable handle for sending reports to a collector.
///
/// [`ErrorReporter::capture`] is the whole public surface, and it is
/// deliberately synchronous, non-blocking and infallible: a `tracing::Layer`
/// cannot await, and a panic hook may run on a thread that has no runtime at
/// all. Anything that could block or fail belongs behind the queue, not in
/// front of it.
#[derive(Clone, Default)]
pub enum ErrorReporter {
    /// Reporting is off, or no collector is configured. Every call is a no-op.
    #[default]
    Disabled,
    /// Hand off to the background sender.
    Remote(mpsc::Sender<CapturedError>),
}

impl std::fmt::Debug for ErrorReporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => f.write_str("ErrorReporter::Disabled"),
            Self::Remote(_) => f.write_str("ErrorReporter::Remote"),
        }
    }
}

impl ErrorReporter {
    /// A reporter that discards everything.
    #[must_use]
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    /// Whether reports actually go anywhere.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Remote(_))
    }

    /// Build a reporter from configuration, spawning the sender task.
    ///
    /// Returns [`ErrorReporter::Disabled`] when reporting is switched off or no
    /// `collector_url` is set — a fresh application never phones home by
    /// accident.
    ///
    /// # Panics
    ///
    /// Panics if called outside a Tokio runtime, like any other `tokio::spawn`.
    #[must_use]
    pub fn start(
        config: &ErrorReportingConfig,
        app_info: AppInfo,
        environment: Environment,
    ) -> Self {
        Self::start_with_shutdown(config, app_info, environment, crate::shutdown::never()).0
    }

    /// Like [`Self::start`], but the sender drains and exits on shutdown.
    ///
    /// Returns the sender's join handle so the boot path can wait for that
    /// final drain — the errors that caused a shutdown are exactly the ones
    /// worth not losing.
    ///
    /// # Panics
    ///
    /// Panics if called outside a Tokio runtime.
    #[must_use]
    pub fn start_with_shutdown(
        config: &ErrorReportingConfig,
        app_info: AppInfo,
        environment: Environment,
        shutdown: crate::shutdown::Shutdown,
    ) -> (Self, Option<tokio::task::JoinHandle<()>>) {
        if !config.is_active() {
            return (Self::Disabled, None);
        }

        let sender_config = Arc::new(SenderConfig {
            endpoint: config.ingest_endpoint(),
            token: config.ingest_token.clone(),
            batch_size: config.batch_size.max(1),
            flush_interval: Duration::from_millis(config.flush_interval_ms.max(1)),
            request_timeout: Duration::from_millis(config.request_timeout_ms.max(1)),
            circuit_breaker_failures: config.circuit_breaker_failures,
            circuit_breaker_cooldown: Duration::from_millis(config.circuit_breaker_cooldown_ms),
            // The *application's* version, not erno's — an operator triaging a
            // regression needs to know which of their deploys introduced it.
            release: Some(app_info.version.to_string()),
            environment: Some(environment.to_string()),
        });

        let (tx, rx) = mpsc::channel(config.queue_capacity.max(1));
        let handle = tokio::spawn(sender_loop(sender_config, rx, shutdown));
        (Self::Remote(tx), Some(handle))
    }

    /// Queue a report.
    ///
    /// Never blocks, never awaits, never fails. A full queue sheds the newest
    /// report and counts it: under a runaway loop the queue is already full of
    /// the same fingerprint, so the newest one carries nothing new.
    pub fn capture(&self, error: CapturedError) {
        let Self::Remote(tx) = self else {
            return;
        };

        match tx.try_send(error) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                metrics::counter!("erno_error_reports_dropped_total", "reason" => "queue_full")
                    .increment(1);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                metrics::counter!("erno_error_reports_dropped_total", "reason" => "closed")
                    .increment(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error_reporting::{Level, Source};

    fn report() -> CapturedError {
        CapturedError::new(Source::Api, Level::Error, "E".to_string(), "m".to_string())
    }

    #[test]
    fn a_disabled_reporter_swallows_everything_without_panicking() {
        let reporter = ErrorReporter::disabled();
        assert!(!reporter.is_active());
        reporter.capture(report());
    }

    #[test]
    fn no_collector_url_means_disabled_even_when_enabled() {
        // The default posture: a fresh app must not try to phone home.
        let config = ErrorReportingConfig::default();
        assert!(config.enabled);
        let reporter = ErrorReporter::start(
            &config,
            AppInfo::new("test", "1.0.0", ""),
            Environment::Test,
        );
        assert!(!reporter.is_active());
    }

    #[tokio::test]
    async fn capture_never_blocks_once_the_queue_is_full() {
        // The property that keeps a collector outage from touching request
        // latency: filling the queue must be cheap and must not await.
        let (tx, _rx) = mpsc::channel(2);
        let reporter = ErrorReporter::Remote(tx);

        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            reporter.capture(report());
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "capture must not block when the queue is saturated"
        );
    }

    #[tokio::test]
    async fn capture_on_a_closed_channel_is_harmless() {
        let (tx, rx) = mpsc::channel(4);
        drop(rx);
        ErrorReporter::Remote(tx).capture(report());
    }
}
