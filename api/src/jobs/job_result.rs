use std::fmt::{Display, Formatter, Result};

use crate::database::models::job_status::JobStatus;
use crate::jobs::JobError;

pub enum JobResult {
    Completed,
    Failed(JobError),
    TimedOut,
}

impl Display for JobResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Completed => write!(f, "completed"),
            Self::Failed(e) => write!(f, "error: {e}"),
            Self::TimedOut => write!(f, "timed out"),
        }
    }
}

impl From<JobResult> for JobStatus {
    fn from(result: JobResult) -> Self {
        match result {
            JobResult::Completed => Self::Completed,
            JobResult::Failed(_) | JobResult::TimedOut => Self::Failed,
        }
    }
}
