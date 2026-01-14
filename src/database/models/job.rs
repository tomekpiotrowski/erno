//! `SeaORM` Entity for job management

use crate::database::models::job_status::JobStatus;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "job")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    pub r#type: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub arguments: Json,
    pub status: JobStatus,
    pub retry_count: i32,
    pub next_execution_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::job_execution::Entity")]
    JobExecution,
}

impl Related<super::job_execution::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::JobExecution.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[allow(dead_code)]
impl Model {
    /// Mark the job as running
    pub const fn start(&mut self) {
        self.status = JobStatus::Running;
    }

    /// Mark the job as completed
    pub const fn complete(&mut self) {
        self.status = JobStatus::Completed;
    }

    /// Mark the job as failed and increment retry count
    pub const fn fail(&mut self) {
        self.status = JobStatus::Failed;
        self.retry_count += 1;
    }

    /// Mark the job as failed and schedule for retry with exponential backoff
    pub fn fail_with_retry(&mut self, base_delay_seconds: u64, multiplier: f64) {
        self.status = JobStatus::Failed;
        self.retry_count += 1;

        // Calculate next execution time using exponential backoff
        #[allow(
            clippy::cast_possible_wrap,
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation
        )]
        let delay_seconds = base_delay_seconds as f64 * multiplier.powi(self.retry_count - 1);
        #[allow(clippy::cast_possible_truncation)]
        let delay_duration = chrono::Duration::seconds(delay_seconds.round() as i64);
        self.next_execution_at = Some(chrono::Utc::now().naive_utc() + delay_duration);
    }

    /// Mark the job as timed out and increment retry count
    pub const fn timeout(&mut self) {
        self.status = JobStatus::Failed;
        self.retry_count += 1;
    }

    /// Mark the job as timed out and schedule for retry with exponential backoff
    pub fn timeout_with_retry(&mut self, base_delay_seconds: u64, multiplier: f64) {
        self.status = JobStatus::Failed;
        self.retry_count += 1;

        // Calculate next execution time using exponential backoff
        #[allow(
            clippy::cast_possible_wrap,
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation
        )]
        let delay_seconds = base_delay_seconds as f64 * multiplier.powi(self.retry_count - 1);
        #[allow(clippy::cast_possible_truncation)]
        let delay_duration = chrono::Duration::seconds(delay_seconds.round() as i64);
        self.next_execution_at = Some(chrono::Utc::now().naive_utc() + delay_duration);
    }

    /// Check if job can be retried based on retry count
    #[must_use]
    pub fn can_retry(&self, max_retries: i32) -> bool {
        self.status == JobStatus::Failed && self.retry_count < max_retries
    }

    /// Check if job is ready for execution (time has passed)
    #[must_use]
    pub fn is_ready_for_execution(&self) -> bool {
        self.next_execution_at
            .is_none_or(|next_execution_at| chrono::Utc::now().naive_utc() >= next_execution_at)
    }

    /// Reset job for retry attempt
    pub const fn reset_for_retry(&mut self) {
        self.status = JobStatus::Pending;
        self.next_execution_at = None;
    }
}
