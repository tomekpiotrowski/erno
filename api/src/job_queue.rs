use std::sync::{Arc, Mutex};

use crate::jobs::Job;

/// Job queue that can be either real (database) or mock (in-memory) for testing
#[derive(Clone, Debug)]
pub enum JobQueue {
    /// Real scheduler that inserts jobs into the database
    Database,
    /// Mock scheduler that captures scheduled jobs for testing
    Mock(Arc<Mutex<Vec<EnqueuedJob>>>),
}

/// A job that was added (captured by mock queue)
#[derive(Debug, Clone)]
pub struct EnqueuedJob {
    pub job_type: String,
    pub arguments: serde_json::Value,
}

impl JobQueue {
    /// Create a new mock queue for testing
    pub fn mock() -> Self {
        Self::Mock(Arc::new(Mutex::new(Vec::new())))
    }

    /// Create a real database queue for production
    pub fn database() -> Self {
        Self::Database
    }

    /// Schedule a typed job. Accepts any `ConnectionTrait`, so it works with a
    /// `DatabaseConnection` or a `DatabaseTransaction` (for atomic enqueues).
    pub async fn add<J, ExtraConfig>(
        &self,
        conn: &impl sea_orm::ConnectionTrait,
        arguments: J::Arguments,
    ) -> Result<(), sea_orm::DbErr>
    where
        J: Job<ExtraConfig>,
        J::Arguments: serde::Serialize,
    {
        self.enqueue_by_name(conn, J::name(), serde_json::to_value(arguments).unwrap())
            .await
    }

    /// Schedule a job by its registered type name with a raw JSON payload.
    ///
    /// Useful where the concrete `Job` type isn't available (e.g. enqueuing from
    /// the admin TUI, or a shared routine that can't name the generic). Accepts
    /// any `ConnectionTrait` so it can run inside a transaction.
    pub async fn enqueue_by_name(
        &self,
        conn: &impl sea_orm::ConnectionTrait,
        job_type: &str,
        arguments: serde_json::Value,
    ) -> Result<(), sea_orm::DbErr> {
        match self {
            Self::Database => {
                // Real implementation - insert into database
                use crate::database::models::{job, job_status::JobStatus};
                use sea_orm::ActiveModelTrait;

                let job_model = job::ActiveModel {
                    id: sea_orm::Set(uuid::Uuid::new_v4()),
                    created_at: sea_orm::NotSet,
                    updated_at: sea_orm::NotSet,
                    r#type: sea_orm::Set(job_type.to_string()),
                    arguments: sea_orm::Set(arguments),
                    status: sea_orm::Set(JobStatus::Pending),
                    retry_count: sea_orm::Set(0),
                    next_execution_at: sea_orm::Set(None),
                };

                job_model.insert(conn).await?;
                Ok(())
            }
            Self::Mock(scheduled) => {
                // Mock implementation - capture the job
                scheduled.lock().unwrap().push(EnqueuedJob {
                    job_type: job_type.to_string(),
                    arguments,
                });
                Ok(())
            }
        }
    }

    /// Get all enqueued jobs (only available for mock queue)
    pub fn enqueued_jobs(&self) -> Option<Vec<EnqueuedJob>> {
        match self {
            Self::Mock(scheduled) => Some(scheduled.lock().unwrap().clone()),
            Self::Database => None,
        }
    }

    /// Get enqueued jobs of a specific type (only available for mock queue)
    pub fn enqueued_jobs_of_type(&self, job_type: &str) -> Option<Vec<EnqueuedJob>> {
        match self {
            Self::Mock(scheduled) => Some(
                scheduled
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|job| job.job_type == job_type)
                    .cloned()
                    .collect(),
            ),
            Self::Database => None,
        }
    }

    /// Clear all scheduled jobs (only available for mock queue)
    pub fn clear_scheduled_jobs(&self) {
        if let Self::Mock(scheduled) = self {
            scheduled.lock().unwrap().clear();
        }
    }
}
