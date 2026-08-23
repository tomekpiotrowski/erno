//! Storing and managing alert rules.
//!
//! Docs: docs/src/content/docs/monitoring/alerts.md

use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder,
};
use serde::Deserialize;
use uuid::Uuid;

use super::rules::{Comparator, RuleSource, RuleState};
use crate::error_reporting::collector::models::alert_rule;

/// Creating a rule.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateRule {
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub comparator: Option<String>,
    pub threshold: f64,
    #[serde(default)]
    pub window_seconds: Option<i64>,
    #[serde(default)]
    pub for_seconds: Option<i64>,
    #[serde(default)]
    pub repeat_seconds: Option<i64>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub notify_email: Option<String>,
    #[serde(default)]
    pub notify_webhook: Option<String>,
}

/// Why a rule could not be saved.
#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    /// The submitted values do not describe a usable rule.
    #[error("{0}")]
    Invalid(String),
    /// The database rejected the write.
    #[error(transparent)]
    Db(#[from] DbErr),
}

/// Create a rule.
///
/// # Errors
///
/// [`RuleError::Invalid`] when the input is unusable, otherwise the database error.
pub async fn create(
    db: &DatabaseConnection,
    input: CreateRule,
) -> Result<alert_rule::Model, RuleError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(RuleError::Invalid("name is required".to_string()));
    }
    let source = RuleSource::from_str_opt(&input.source).ok_or_else(|| {
        RuleError::Invalid("source must be errors, uptime, or subsystem".to_string())
    })?;
    if !input.threshold.is_finite() {
        return Err(RuleError::Invalid("threshold must be a number".to_string()));
    }

    let model = alert_rule::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(truncate(name, 200)),
        enabled: Set(true),
        source: Set(source.as_str().to_string()),
        selector: Set(input
            .selector
            .map(|s| truncate(&s, 200))
            .unwrap_or_default()),
        comparator: Set(
            Comparator::from_str_or_gt(input.comparator.as_deref().unwrap_or("gt"))
                .as_str()
                .to_string(),
        ),
        threshold: Set(input.threshold),
        window_seconds: Set(input.window_seconds.unwrap_or(300).clamp(30, 86_400)),
        // Zero is meaningful — "fire immediately" — so only the top is clamped.
        for_seconds: Set(input.for_seconds.unwrap_or(120).clamp(0, 86_400)),
        repeat_seconds: Set(input.repeat_seconds.unwrap_or(14_400).clamp(0, 604_800)),
        severity: Set(normalize_severity(input.severity.as_deref())),
        notify_email: Set(input.notify_email.filter(|e| !e.trim().is_empty())),
        notify_webhook: Set(input.notify_webhook.filter(|u| !u.trim().is_empty())),
        silence_until: Set(None),
        state: Set(RuleState::Ok.as_str().to_string()),
        state_since: Set(None),
        last_notified_at: Set(None),
        last_evaluated_at: Set(None),
        last_value: Set(None),
        created_at: Set(Utc::now().naive_utc()),
    };

    Ok(model.insert(db).await?)
}

fn normalize_severity(severity: Option<&str>) -> String {
    match severity {
        Some("critical") => "critical",
        Some("info") => "info",
        _ => "warning",
    }
    .to_string()
}

/// Every rule, worst first so what is firing is at the top.
///
/// # Errors
///
/// Returns the database error.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<alert_rule::Model>, DbErr> {
    let mut rules = alert_rule::Entity::find()
        .order_by_asc(alert_rule::Column::Name)
        .all(db)
        .await?;

    rules.sort_by_key(|r| match RuleState::from_str_or_ok(&r.state) {
        RuleState::Firing => 0,
        RuleState::Pending => 1,
        RuleState::Ok => 2,
    });
    Ok(rules)
}

/// Rules the evaluator should consider.
///
/// # Errors
///
/// Returns the database error.
pub async fn enabled(db: &DatabaseConnection) -> Result<Vec<alert_rule::Model>, DbErr> {
    alert_rule::Entity::find()
        .filter(alert_rule::Column::Enabled.eq(true))
        .all(db)
        .await
}

/// Delete a rule.
///
/// # Errors
///
/// Returns the database error.
pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<bool, DbErr> {
    let result = alert_rule::Entity::delete_by_id(id).exec(db).await?;
    Ok(result.rows_affected > 0)
}

/// Turn a rule on or off.
///
/// # Errors
///
/// Returns the database error.
pub async fn set_enabled(
    db: &DatabaseConnection,
    id: Uuid,
    enabled: bool,
) -> Result<Option<alert_rule::Model>, DbErr> {
    let Some(model) = alert_rule::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: alert_rule::ActiveModel = model.into();
    active.enabled = Set(enabled);
    Ok(Some(active.update(db).await?))
}

/// Silence a rule for a number of minutes.
///
/// Silencing suppresses notifications but not evaluation — the console still
/// shows the truth, which is what an operator needs while working on it.
///
/// # Errors
///
/// Returns the database error.
pub async fn silence(
    db: &DatabaseConnection,
    id: Uuid,
    minutes: i64,
) -> Result<Option<alert_rule::Model>, DbErr> {
    let Some(model) = alert_rule::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: alert_rule::ActiveModel = model.into();
    active.silence_until = Set(if minutes <= 0 {
        None
    } else {
        Some(Utc::now().naive_utc() + Duration::minutes(minutes.min(60 * 24 * 30)))
    });
    Ok(Some(active.update(db).await?))
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_falls_back_to_warning() {
        assert_eq!(normalize_severity(Some("critical")), "critical");
        assert_eq!(normalize_severity(Some("info")), "info");
        assert_eq!(normalize_severity(Some("nonsense")), "warning");
        assert_eq!(normalize_severity(None), "warning");
    }
}
