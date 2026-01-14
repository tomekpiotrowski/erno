//! `SeaORM` Entity for job execution tracking

use crate::database::models::job_result::JobResult;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "job_execution")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub job_id: Uuid,
    pub result: JobResult,
    pub started_at: DateTime,
    pub finished_at: DateTime,
    pub execution_time_ms: i64,
    pub failure_reason: Option<String>,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::job::Entity",
        from = "Column::JobId",
        to = "super::job::Column::Id"
    )]
    Job,
}

impl Related<super::job::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Job.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[allow(dead_code)]
impl Model {
    /// Calculate execution duration in milliseconds
    pub const fn execution_duration(&self) -> chrono::Duration {
        self.finished_at.signed_duration_since(self.started_at)
    }

    /// Check if this execution was successful
    pub const fn was_successful(&self) -> bool {
        self.result.is_successful()
    }

    /// Check if this execution failed
    pub const fn was_failed(&self) -> bool {
        self.result.is_failed()
    }

    /// Check if this execution timed out
    pub const fn was_timed_out(&self) -> bool {
        self.result.is_timed_out()
    }
}
