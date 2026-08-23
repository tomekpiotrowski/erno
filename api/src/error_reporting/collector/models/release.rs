//! Docs: docs/src/content/docs/monitoring/releases.md
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// One deploy of one version to one environment.
///
/// Recorded by a CI pipeline. This is what makes every other signal
/// interpretable — "the error rate climbed at 14:02" is far more useful when
/// something says a deploy landed at 14:01.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "release")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub version: String,
    pub environment: String,
    pub commit_sha: Option<String>,
    /// Who recorded it — `github-actions`, `manual`, and so on.
    pub source: Option<String>,
    pub deployed_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
