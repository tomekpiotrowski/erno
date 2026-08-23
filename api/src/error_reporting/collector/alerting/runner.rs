//! The alert evaluation loop.
//!
//! Docs: docs/src/content/docs/monitoring/alerts.md
//!
//! Runs under a deployment-wide advisory lock: replicas each evaluating and
//! each notifying would multiply every alert by the replica count, which is the
//! fastest possible route to people muting the whole system.

use std::time::Duration;

use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection};

use super::{
    evaluator::{observe, ObserveContext},
    notifier::{self, NotifyContext},
    rules::{advance, Comparator, Notify, RuleState, RuleStatus, RuleTiming},
    service,
};
use crate::{
    error_reporting::collector::models::alert_rule,
    health::HealthThresholds,
    jobs::advisory_lock::{lock_keys, run_with_advisory_lock},
    mailer::Mailer,
};

/// How often rules are evaluated. Individual rules express patience through
/// `for_seconds` rather than through this.
const TICK: Duration = Duration::from_secs(30);

/// Start evaluating alert rules.
pub fn spawn(
    db: DatabaseConnection,
    mailer: Mailer,
    context: NotifyContext,
    thresholds: HealthThresholds,
    prometheus_url: Option<String>,
) {
    tokio::spawn(async move {
        run_with_advisory_lock(db, lock_keys::ALERTING, "alert rules", move |db| {
            let mailer = mailer.clone();
            let context = context.clone();
            let thresholds = thresholds.clone();
            let prometheus_url = prometheus_url.clone();
            async move {
                let client = match reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()
                {
                    Ok(client) => client,
                    Err(e) => {
                        eprintln!("alerting: could not build HTTP client: {e}");
                        return;
                    }
                };

                loop {
                    tokio::time::sleep(TICK).await;
                    if let Err(e) = evaluate_all(
                        &db,
                        &mailer,
                        &client,
                        &context,
                        &thresholds,
                        prometheus_url.as_deref(),
                    )
                    .await
                    {
                        eprintln!("alerting: evaluation pass failed: {e}");
                    }
                }
            }
        })
        .await;
    });
}

/// Evaluate every enabled rule once.
///
/// # Errors
///
/// Returns the database error if rules could not be loaded or stored.
pub async fn evaluate_all(
    db: &DatabaseConnection,
    mailer: &Mailer,
    client: &reqwest::Client,
    context: &NotifyContext,
    thresholds: &HealthThresholds,
    prometheus_url: Option<&str>,
) -> Result<(), sea_orm::DbErr> {
    let rules = service::enabled(db).await?;
    let now = Utc::now().naive_utc();

    // Built once per pass: every rule reads from the same sources.
    let ctx = ObserveContext {
        db,
        thresholds,
        http: client,
        prometheus_url,
    };

    for rule in rules {
        let observation = match observe(&ctx, &rule).await {
            Ok(observation) => observation,
            Err(e) => {
                // One broken rule must not stop the others being evaluated.
                eprintln!("alerting: rule {:?} could not be evaluated: {e}", rule.name);
                continue;
            }
        };

        let breaching = Comparator::from_str_or_gt(&rule.comparator)
            .breaches(observation.value, rule.threshold);

        let transition = advance(
            RuleStatus {
                state: RuleState::from_str_or_ok(&rule.state),
                state_since: rule.state_since,
                last_notified_at: rule.last_notified_at,
                silence_until: rule.silence_until,
            },
            RuleTiming {
                for_seconds: rule.for_seconds,
                repeat_seconds: rule.repeat_seconds,
            },
            breaching,
            now,
        );

        if transition.notify != Notify::Nothing {
            notifier::send(
                mailer,
                client,
                context,
                &rule,
                transition.notify,
                &observation.description,
            )
            .await;

            metrics::counter!(
                "erno_alerts_fired_total",
                "rule" => rule.name.clone(),
                "kind" => match transition.notify {
                    Notify::Firing => "firing",
                    Notify::Resolved => "resolved",
                    Notify::Nothing => "nothing",
                }
            )
            .increment(1);

            tracing::info!(
                target: "erno::error_reporting::collector",
                "🔔 Alert {} — {}: {}",
                match transition.notify {
                    Notify::Firing => "firing",
                    Notify::Resolved => "resolved",
                    Notify::Nothing => "",
                },
                rule.name,
                observation.description
            );
        }

        let notified_now = transition.notify != Notify::Nothing;
        let mut active: alert_rule::ActiveModel = rule.into();
        active.state = Set(transition.state.as_str().to_string());
        active.state_since = Set(transition.state_since);
        active.last_evaluated_at = Set(Some(now));
        active.last_value = Set(Some(observation.description));
        if notified_now {
            active.last_notified_at = Set(Some(now));
        }
        active.update(db).await?;
    }

    Ok(())
}
