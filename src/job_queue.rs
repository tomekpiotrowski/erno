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

    /// Schedule a job
    pub async fn add<J: Job>(
        &self,
        db: &sea_orm::DatabaseConnection,
        arguments: J::Arguments,
    ) -> Result<(), sea_orm::DbErr>
    where
        J::Arguments: serde::Serialize,
    {
        match self {
            Self::Database => {
                // Real implementation - insert into database
                use crate::database::models::{job, job_status::JobStatus};
                use sea_orm::ActiveModelTrait;

                let job_id = uuid::Uuid::new_v4();

                let job_model = job::ActiveModel {
                    id: sea_orm::Set(job_id),
                    created_at: sea_orm::NotSet,
                    updated_at: sea_orm::NotSet,
                    r#type: sea_orm::Set(J::name().to_string()),
                    arguments: sea_orm::Set(serde_json::to_value(arguments).unwrap()),
                    status: sea_orm::Set(JobStatus::Pending),
                    retry_count: sea_orm::Set(0),
                    next_execution_at: sea_orm::Set(None),
                };

                job_model.insert(db).await?;
                Ok(())
            }
            Self::Mock(scheduled) => {
                // Mock implementation - capture the job
                scheduled.lock().unwrap().push(EnqueuedJob {
                    job_type: J::name().to_string(),
                    arguments: serde_json::to_value(arguments).unwrap(),
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
