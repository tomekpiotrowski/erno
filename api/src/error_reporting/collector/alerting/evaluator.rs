//! Reading the number a rule compares against.
//!
//! Docs: docs/src/content/docs/monitoring/alerts.md

use chrono::{Duration, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, Statement, Value,
};

use super::rules::RuleSource;
use crate::error_reporting::collector::models::{alert_rule, app_health, uptime_check};
use crate::health::{HealthSnapshot, HealthState, HealthThresholds};

/// What a rule observed, plus how to say it.
#[derive(Debug, Clone)]
pub struct Observation {
    /// The number compared with the threshold.
    pub value: f64,
    /// A phrase for the notification, e.g. "3 issues in the last 5m".
    pub description: String,
}

/// Evaluate one rule against current data.
///
/// # Errors
///
/// Returns the database error when a query fails.
pub async fn observe(
    db: &DatabaseConnection,
    rule: &alert_rule::Model,
    thresholds: &HealthThresholds,
) -> Result<Observation, DbErr> {
    match RuleSource::from_str_opt(&rule.source) {
        Some(RuleSource::Errors) => observe_errors(db, rule).await,
        Some(RuleSource::Uptime) => observe_uptime(db, rule).await,
        Some(RuleSource::Subsystem) => observe_subsystem(db, rule, thresholds).await,
        // An unrecognised source must read as "nothing wrong" rather than
        // firing on a typo.
        None => Ok(Observation {
            value: 0.0,
            description: format!("unknown source {:?}", rule.source),
        }),
    }
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

    // Otherwise: event volume, optionally narrowed to one source.
    let mut sql =
        String::from("SELECT count(*)::bigint AS value FROM error_event WHERE created_at >= $1");
    let mut values: Vec<Value> = vec![Value::ChronoDateTime(Some(Box::new(since)))];
    if !rule.selector.is_empty() && rule.selector != "all" {
        values.push(rule.selector.clone().into());
        sql.push_str(" AND source = $2");
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
    let rows = app_health::Entity::find().all(db).await?;
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
    use super::*;

    #[test]
    fn windows_read_the_way_an_operator_expects() {
        assert_eq!(humanize_window(30), "30s");
        assert_eq!(humanize_window(300), "5m");
        assert_eq!(humanize_window(7_200), "2h");
        assert_eq!(humanize_window(172_800), "2d");
    }
}
