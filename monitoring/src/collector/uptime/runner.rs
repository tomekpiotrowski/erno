//! The probe scheduler.
//!
//! Docs: docs/src/content/docs/monitoring/uptime.md
//!
//! Runs under a deployment-wide advisory lock so replicas do not all probe the
//! same targets — which would multiply load on whatever is being checked and
//! make the recorded ratios meaningless.

use std::time::Duration;

use sea_orm::DatabaseConnection;

use super::{probe, service};
use erno::jobs::advisory_lock::{lock_keys, run_with_advisory_lock};

/// How often the runner looks for due checks. Individual checks have their own
/// intervals; this is just the polling granularity.
const TICK: Duration = Duration::from_secs(10);
/// How long raw probe results are kept.
const RESULT_RETENTION_DAYS: i64 = 7;
/// Ticks between result pruning passes.
const PRUNE_EVERY_TICKS: u32 = 360;

/// Start probing.
pub fn spawn(db: DatabaseConnection) {
    tokio::spawn(async move {
        run_with_advisory_lock(db, lock_keys::UPTIME, "uptime checks", |db| async move {
            let client = match reqwest::Client::builder()
                // Redirects would let a check pass by landing somewhere else
                // entirely, which is not what an operator asked to verify.
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("erno-monitoring/", env!("CARGO_PKG_VERSION")))
                .build()
            {
                Ok(client) => client,
                Err(e) => {
                    eprintln!("uptime: could not build HTTP client: {e}");
                    return;
                }
            };

            let mut ticks: u32 = 0;
            loop {
                tokio::time::sleep(TICK).await;
                ticks = ticks.wrapping_add(1);

                match service::due(&db).await {
                    Ok(checks) => {
                        for check in checks {
                            let outcome = probe::run(&client, &check).await;
                            match service::record_probe(&db, &check, &outcome).await {
                                Ok(changed) => {
                                    if changed {
                                        // A transition is the only moment worth
                                        // telling anyone about.
                                        tracing::info!(
                                            target: erno::error_reporting::COLLECTOR_TARGET,
                                            "🔔 Uptime check '{}' is now {}",
                                            check.name,
                                            if outcome.ok { "up" } else { "down" }
                                        );
                                        metrics::counter!(
                                            "erno_uptime_transitions_total",
                                            "state" => if outcome.ok { "up" } else { "down" }
                                        )
                                        .increment(1);
                                    }
                                }
                                Err(e) => eprintln!("uptime: could not record probe: {e}"),
                            }
                        }
                    }
                    Err(e) => eprintln!("uptime: could not list due checks: {e}"),
                }

                if ticks.is_multiple_of(PRUNE_EVERY_TICKS) {
                    if let Err(e) = service::prune_results(&db, RESULT_RETENTION_DAYS).await {
                        eprintln!("uptime: could not prune results: {e}");
                    }
                }
            }
        })
        .await;
    });
}
