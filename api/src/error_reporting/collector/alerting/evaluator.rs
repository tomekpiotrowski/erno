//! Reading the number a rule compares against.
//!
//! Docs: docs/src/content/docs/monitoring/alerts.md

use std::collections::HashMap;

use chrono::{Duration, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, Statement, Value,
};

use super::rules::RuleSource;
use crate::error_reporting::collector::models::{alert_rule, app_health, uptime_check};
use crate::health::{HealthSnapshot, HealthState, HealthThresholds};
use uuid::Uuid;

/// What a rule observed, plus how to say it.
#[derive(Debug, Clone)]
pub struct Observation {
    /// The number compared with the threshold.
    pub value: f64,
    /// A phrase for the notification, e.g. "3 issues in the last 5m".
    pub description: String,
}

/// What [`observe`] needs. A struct rather than five positional arguments, and
/// built once per evaluation pass by the runner.
pub struct ObserveContext<'a> {
    pub db: &'a DatabaseConnection,
    pub thresholds: &'a HealthThresholds,
    /// Shared with the notifier; the runner already builds one.
    pub http: &'a reqwest::Client,
    /// Base URL of the bundled Prometheus. `None` disables the PromQL source.
    pub prometheus_url: Option<&'a str>,
    /// Project id to slug, built once per pass. PromQL rules are checked
    /// against their own project's slug, which is not on the rule row.
    pub project_slugs: &'a HashMap<Uuid, String>,
}

/// Evaluate one rule against current data.
///
/// # Errors
///
/// Returns the database error when a query fails.
pub async fn observe(
    ctx: &ObserveContext<'_>,
    rule: &alert_rule::Model,
) -> Result<Observation, DbErr> {
    match RuleSource::from_str_opt(&rule.source) {
        Some(RuleSource::Errors) => observe_errors(ctx.db, rule).await,
        Some(RuleSource::Uptime) => observe_uptime(ctx.db, rule).await,
        Some(RuleSource::Subsystem) => observe_subsystem(ctx.db, rule, ctx.thresholds).await,
        Some(RuleSource::Promql) => Ok(observe_promql(ctx, rule).await),
        // An unrecognised source must read as "nothing wrong" rather than
        // firing on a typo.
        None => Ok(Observation {
            value: 0.0,
            description: format!("unknown source {:?}", rule.source),
        }),
    }
}

/// Run the rule's selector as an instant PromQL query.
///
/// Infallible by design: `observe` returns `DbErr`, and a Prometheus failure is
/// not one. A failed or empty query reads as **not breaching**, consistent with
/// the unknown-source arm.
///
/// That has a cost worth stating plainly: while Prometheus is unreachable,
/// every PromQL rule silently stops firing. The mitigation is the counter
/// below — the collector's own `/metrics` is scraped by that same Prometheus,
/// so `erno_alert_source_unavailable_total` is itself alertable, and an
/// operator can build the one rule that catches the blind spot.
async fn observe_promql(ctx: &ObserveContext<'_>, rule: &alert_rule::Model) -> Observation {
    let unavailable = |reason: &str| {
        metrics::counter!("erno_alert_source_unavailable_total", "source" => "promql").increment(1);
        Observation {
            value: 0.0,
            description: format!("PromQL unavailable ({reason})"),
        }
    };
    // Same counter, a different label value, so the one catch-all rule an
    // operator writes over `erno_alert_source_unavailable_total` also catches
    // a selector that forgot its project.
    let unscoped = |wanted: &str| {
        metrics::counter!("erno_alert_source_unavailable_total", "source" => "promql_unscoped")
            .increment(1);
        Observation {
            value: 0.0,
            description: format!("PromQL selector is not scoped to this project (needs {wanted})"),
        }
    };

    let Some(base) = ctx.prometheus_url.filter(|u| !u.trim().is_empty()) else {
        return unavailable("no prometheus url configured");
    };
    if rule.selector.trim().is_empty() {
        return Observation {
            value: 0.0,
            description: "PromQL rule has no query".to_string(),
        };
    }

    // Prometheus holds every project's metrics, so an unscoped selector counts
    // the whole organisation and fires this project's alert on another app's
    // traffic. Requiring the matcher as a literal substring is deliberate: the
    // alternative is injecting it into arbitrary PromQL (`rate(...)`, `or`,
    // `ignoring(...)`), which needs a parser this repository does not have.
    // The console's rule editor inserts it.
    let Some(slug) = ctx.project_slugs.get(&rule.project_id) else {
        return unscoped("project has no slug");
    };
    let matcher = format!("erno_project=\"{slug}\"");
    if !rule.selector.contains(&matcher) {
        return unscoped(&matcher);
    }

    let url = format!("{}/api/v1/query", base.trim_end_matches('/'));
    let response = match ctx
        .http
        .get(&url)
        .query(&[("query", rule.selector.as_str())])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return unavailable(&e.to_string()),
    };
    if !response.status().is_success() {
        return unavailable(&format!("status {}", response.status()));
    }
    let Ok(body) = response.text().await else {
        return unavailable("unreadable response");
    };

    match scalar_from_instant_response(&body) {
        Some(value) => Observation {
            value,
            description: format!("{} = {value}", truncate_for_display(&rule.selector)),
        },
        // An empty result is a real answer from a healthy Prometheus — the
        // series simply has no samples — so it is not counted as unavailable.
        None => Observation {
            value: 0.0,
            description: format!(
                "{} returned no samples",
                truncate_for_display(&rule.selector)
            ),
        },
    }
}

/// Pull the single scalar out of a Prometheus instant-query response.
///
/// Split from the request so it can be asserted against fixtures without a
/// server, the same split as `render_config` in the CLI.
#[must_use]
pub fn scalar_from_instant_response(body: &str) -> Option<f64> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    if parsed.get("status").and_then(|s| s.as_str()) != Some("success") {
        return None;
    }
    let result = parsed.get("data")?.get("result")?.as_array()?;
    // `value` for an instant vector or scalar, `values` for a matrix — take the
    // last sample in that case, which is the most recent.
    let first = result.first()?;
    let sample = first
        .get("value")
        .or_else(|| first.get("values").and_then(|v| v.as_array()?.last()))?;
    // Prometheus renders sample values as strings, including "NaN" and "+Inf".
    let raw = sample.as_array()?.get(1)?.as_str()?;
    raw.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn truncate_for_display(query: &str) -> String {
    if query.chars().count() <= 60 {
        return query.to_string();
    }
    let head: String = query.chars().take(57).collect();
    format!("{head}...")
}

async fn observe_errors(
    db: &DatabaseConnection,
    rule: &alert_rule::Model,
) -> Result<Observation, DbErr> {
    let since = Utc::now().naive_utc() - Duration::seconds(rule.window_seconds.max(1));
    let window = humanize_window(rule.window_seconds);

    // `new_issues` is the alert Alertmanager cannot express at all: it needs to
    // know that a fingerprint has never been seen before.
    if rule.selector == "new_issues" {
        let count = crate::error_reporting::collector::models::error_issue::Entity::find()
            .filter(
                crate::error_reporting::collector::models::error_issue::Column::ProjectId
                    .eq(rule.project_id),
            )
            .filter(
                crate::error_reporting::collector::models::error_issue::Column::FirstSeen
                    .gte(since),
            )
            .count(db)
            .await?;
        return Ok(Observation {
            value: count as f64,
            description: format!("{count} new error type(s) in the last {window}"),
        });
    }

    // Otherwise: event volume, optionally narrowed to one source. Placeholder
    // numbers come from `values.len()`, so adding a filter cannot shift the
    // ones written after it.
    let mut sql =
        String::from("SELECT count(*)::bigint AS value FROM error_event WHERE created_at >= $1");
    let mut values: Vec<Value> = vec![Value::ChronoDateTime(Some(Box::new(since)))];
    values.push(rule.project_id.into());
    sql.push_str(&format!(" AND project_id = ${}", values.len()));
    if !rule.selector.is_empty() && rule.selector != "all" {
        values.push(rule.selector.clone().into());
        sql.push_str(&format!(" AND source = ${}", values.len()));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            values,
        ))
        .await?;
    let count: i64 = row.map_or(Ok(0), |r| r.try_get("", "value"))?;

    Ok(Observation {
        value: count as f64,
        description: format!("{count} error event(s) in the last {window}"),
    })
}

async fn observe_uptime(
    db: &DatabaseConnection,
    rule: &alert_rule::Model,
) -> Result<Observation, DbErr> {
    let mut finder = uptime_check::Entity::find()
        .filter(uptime_check::Column::ProjectId.eq(rule.project_id))
        .filter(uptime_check::Column::Enabled.eq(true))
        .filter(uptime_check::Column::CurrentState.eq("down"));

    if !rule.selector.is_empty() && rule.selector != "all" {
        finder = finder.filter(uptime_check::Column::Name.eq(rule.selector.clone()));
    }

    let down = finder.count(db).await?;
    let description = if rule.selector.is_empty() || rule.selector == "all" {
        format!("{down} uptime check(s) down")
    } else {
        format!(
            "check {:?} is {}",
            rule.selector,
            if down > 0 { "down" } else { "up" }
        )
    };

    Ok(Observation {
        value: down as f64,
        description,
    })
}

async fn observe_subsystem(
    db: &DatabaseConnection,
    rule: &alert_rule::Model,
    thresholds: &HealthThresholds,
) -> Result<Observation, DbErr> {
    let rows = app_health::Entity::find()
        .filter(app_health::Column::ProjectId.eq(rule.project_id))
        .all(db)
        .await?;
    let now = Utc::now().naive_utc();

    // "degraded" counts anything that is not fully healthy; the default counts
    // only what is actually down.
    let count_degraded = rule.selector == "degraded";

    let mut unhealthy = 0u64;
    for row in rows {
        let stale = (now - row.reported_at).num_seconds() >= thresholds.heartbeat_stale_seconds;
        let state = if stale {
            HealthState::Down
        } else {
            serde_json::from_value::<HealthSnapshot>(row.payload)
                .map_or(HealthState::Degraded, |s| s.overall(thresholds))
        };

        let counts = match state {
            HealthState::Down => true,
            HealthState::Degraded => count_degraded,
            HealthState::Ok => false,
        };
        if counts {
            unhealthy += 1;
        }
    }

    Ok(Observation {
        value: unhealthy as f64,
        description: format!(
            "{unhealthy} instance(s) {}",
            if count_degraded {
                "not healthy"
            } else {
                "down"
            }
        ),
    })
}

/// Render a window the way an operator reads it.
fn humanize_window(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3_600),
        s => format!("{}d", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::scalar_from_instant_response as scalar;

    #[test]
    fn a_prometheus_instant_vector_yields_its_sample() {
        // Prometheus renders sample values as strings, so a naive as_f64()
        // would silently read every result as "no data".
        let body = r#"{"status":"success","data":{"resultType":"vector",
            "result":[{"metric":{"job":"api"},"value":[1700000000,"0.42"]}]}}"#;
        assert_eq!(scalar(body), Some(0.42));
    }

    #[test]
    fn a_matrix_takes_the_most_recent_sample() {
        let body = r#"{"status":"success","data":{"resultType":"matrix",
            "result":[{"metric":{},"values":[[1,"1"],[2,"7"]]}]}}"#;
        assert_eq!(scalar(body), Some(7.0));
    }

    #[test]
    fn an_empty_result_is_no_value_rather_than_zero() {
        // Zero would be a *reading*, and a `lt` rule would fire on it.
        let body = r#"{"status":"success","data":{"resultType":"vector","result":[]}}"#;
        assert_eq!(scalar(body), None);
    }

    #[test]
    fn an_error_response_yields_nothing() {
        let body = r#"{"status":"error","errorType":"bad_data","error":"parse error"}"#;
        assert_eq!(scalar(body), None);
        assert_eq!(scalar("not json at all"), None);
        assert_eq!(scalar("{}"), None);
    }

    #[test]
    fn non_finite_samples_are_rejected() {
        // Prometheus emits these literally, and NaN compares false against
        // every threshold, which would make a rule quietly undecidable.
        for raw in ["NaN", "+Inf", "-Inf"] {
            let body = format!(
                r#"{{"status":"success","data":{{"result":[{{"metric":{{}},"value":[1,"{raw}"]}}]}}}}"#
            );
            assert_eq!(scalar(&body), None, "{raw} should not be a reading");
        }
    }

    #[test]
    fn promql_is_a_recognised_source_and_round_trips() {
        use crate::error_reporting::collector::alerting::rules::RuleSource;
        assert_eq!(RuleSource::from_str_opt("promql"), Some(RuleSource::Promql));
        assert_eq!(RuleSource::Promql.as_str(), "promql");
    }

    use super::*;

    #[test]
    fn windows_read_the_way_an_operator_expects() {
        assert_eq!(humanize_window(30), "30s");
        assert_eq!(humanize_window(300), "5m");
        assert_eq!(humanize_window(7_200), "2h");
        assert_eq!(humanize_window(172_800), "2d");
    }
}
