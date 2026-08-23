//! Pushing health readings to a monitoring collector.
//!
//! Docs: docs/src/content/docs/monitoring/subsystem-health.md
//!
//! Same constraint as error reporting: this must never be able to hurt the
//! application. The task is entirely off the request path, every request has a
//! timeout, and a failure is counted and forgotten rather than retried — the
//! next reading is along in a moment and is more useful than the stale one.

use std::time::Duration;

use sea_orm::DatabaseConnection;

use super::{gather::gather, snapshot::HealthSnapshot};

/// What the health reporter needs to run.
#[derive(Debug, Clone)]
pub struct HealthReporterConfig {
    /// Absolute URL of the collector's health endpoint.
    pub endpoint: String,
    /// Trusted server-to-server token.
    pub token: String,
    /// How often to report.
    pub interval: Duration,
    /// Hard cap on one request.
    pub request_timeout: Duration,
    /// Identifies this process among its replicas.
    pub instance: String,
    /// Application version.
    pub release: Option<String>,
    /// Deployment environment.
    pub environment: String,
    /// Used to decide whether a `running` job is stuck.
    pub job_timeout_seconds: u32,
}

/// Start reporting health on an interval.
///
/// Returns immediately; the work happens on a spawned task.
pub fn spawn_health_reporter(
    db: DatabaseConnection,
    connections: crate::websocket::connections::Connections,
    config: HealthReporterConfig,
) {
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                // Never `tracing::error!`: the error-capture layer would turn a
                // reporting failure into an error report.
                eprintln!("health: could not build HTTP client: {e}");
                return;
            }
        };

        loop {
            tokio::time::sleep(config.interval).await;

            let snapshot = match gather(
                &db,
                config.instance.clone(),
                config.release.clone(),
                config.environment.clone(),
                config.job_timeout_seconds,
                i64::try_from(connections.user_count().await).unwrap_or(i64::MAX),
            )
            .await
            {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    eprintln!("health: could not gather snapshot: {e}");
                    continue;
                }
            };

            // Publish locally too, so a Prometheus-only deployment gets the
            // same numbers without any collector.
            super::gather::export_gauges(&snapshot);
            send(&client, &config, &snapshot).await;
        }
    });
}

async fn send(client: &reqwest::Client, config: &HealthReporterConfig, snapshot: &HealthSnapshot) {
    let result = client
        .post(&config.endpoint)
        .header(
            crate::error_reporting::collector::auth::INGEST_KEY_HEADER,
            &config.token,
        )
        .json(snapshot)
        .send()
        .await;

    match result {
        Ok(response) if response.status().is_success() => {
            metrics::counter!("erno_health_reports_total", "result" => "sent").increment(1);
        }
        // No retry: the next reading is due shortly and will be more accurate
        // than a replay of this one.
        Ok(response) => {
            metrics::counter!("erno_health_reports_total", "result" => "rejected").increment(1);
            eprintln!("health: collector rejected reading: {}", response.status());
        }
        Err(e) => {
            metrics::counter!("erno_health_reports_total", "result" => "failed").increment(1);
            eprintln!("health: could not reach collector: {e}");
        }
    }
}
