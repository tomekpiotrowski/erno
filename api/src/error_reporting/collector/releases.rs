//! Release tracking.
//!
//! Docs: docs/src/content/docs/monitoring/releases.md
//!
//! A deploy is the single most common explanation for a change in error
//! behaviour, so recording deploys is what turns "issues appeared" into "this
//! deploy introduced them".

use chrono::{DateTime, NaiveDateTime, Utc};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::models::{error_issue, release};

/// Releases returned by the list endpoint at most.
const MAX_LIMIT: u64 = 100;

/// What a CI pipeline posts on deploy.
#[derive(Debug, Clone, Deserialize)]
pub struct RecordRelease {
    /// The version that was deployed.
    pub version: String,
    /// Which environment it went to.
    pub environment: String,
    /// Commit it was built from.
    #[serde(default)]
    pub commit_sha: Option<String>,
    /// Who recorded it.
    #[serde(default)]
    pub source: Option<String>,
    /// When it landed. Defaults to now.
    #[serde(default)]
    pub deployed_at: Option<DateTime<Utc>>,
}

/// A recorded deploy, with what it appears to have introduced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseDto {
    pub id: Uuid,
    pub version: String,
    pub environment: String,
    pub commit_sha: Option<String>,
    pub source: Option<String>,
    pub deployed_at: NaiveDateTime,
    /// Issues whose *first* sighting carried this version.
    pub new_issues: i64,
}

/// A page of releases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseListResponse {
    pub releases: Vec<ReleaseDto>,
}

/// Query parameters for the list endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseQuery {
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub limit: Option<u64>,
}

/// Record a deploy, or update it if the same version and environment is
/// re-posted.
///
/// Re-running a pipeline must not create a second row, so this upserts on
/// `(version, environment)`.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the write fails.
pub async fn record(
    db: &DatabaseConnection,
    input: RecordRelease,
) -> Result<release::Model, DbErr> {
    use sea_orm::sea_query::OnConflict;

    let deployed_at = input
        .deployed_at
        .map_or_else(|| Utc::now().naive_utc(), |ts| ts.naive_utc());

    let model = release::ActiveModel {
        id: Set(Uuid::new_v4()),
        version: Set(truncate(input.version.trim(), 200)),
        environment: Set(truncate(input.environment.trim(), 100)),
        commit_sha: Set(input.commit_sha.map(|s| truncate(&s, 64))),
        source: Set(input.source.map(|s| truncate(&s, 100))),
        deployed_at: Set(deployed_at),
        created_at: Set(Utc::now().naive_utc()),
    };

    release::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([release::Column::Version, release::Column::Environment])
                .update_columns([
                    release::Column::CommitSha,
                    release::Column::Source,
                    release::Column::DeployedAt,
                ])
                .to_owned(),
        )
        .exec_with_returning(db)
        .await
}

/// List recent deploys, newest first, each with the number of issues it
/// appears to have introduced.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when a query fails.
pub async fn list(
    db: &DatabaseConnection,
    query: &ReleaseQuery,
) -> Result<ReleaseListResponse, DbErr> {
    let limit = query.limit.unwrap_or(20).clamp(1, MAX_LIMIT);

    let mut finder = release::Entity::find().order_by_desc(release::Column::DeployedAt);
    if let Some(environment) = query
        .environment
        .as_deref()
        .filter(|e| !e.is_empty() && *e != "all")
    {
        finder = finder.filter(release::Column::Environment.eq(environment));
    }

    let releases = finder.limit(limit).all(db).await?;

    let mut out = Vec::with_capacity(releases.len());
    for model in releases {
        // `first_release` is written only on insert, so this counts issues born
        // in this version rather than merely seen during it.
        let new_issues = error_issue::Entity::find()
            .filter(error_issue::Column::FirstRelease.eq(model.version.clone()))
            .count(db)
            .await? as i64;

        out.push(ReleaseDto {
            id: model.id,
            version: model.version,
            environment: model.environment,
            commit_sha: model.commit_sha,
            source: model.source,
            deployed_at: model.deployed_at,
            new_issues,
        });
    }

    Ok(ReleaseListResponse { releases: out })
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}

use sea_orm::PaginatorTrait;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_fields_are_truncated() {
        assert_eq!(truncate(&"v".repeat(500), 200).chars().count(), 200);
        assert_eq!(truncate("1.2.3", 200), "1.2.3");
    }
}
