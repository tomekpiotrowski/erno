//! Docs: docs/src/content/docs/api/jobs.md
mod advisory_lock;
pub mod job_registry;
pub mod job_result;
pub mod job_supervisor;
pub mod scheduled_job;
mod scheduler;
pub mod send_password_reset_email_job;
pub mod send_verification_email_job;
mod worker;

use crate::app::App;
use serde::de::DeserializeOwned;
use std::future::Future;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JobError {
    #[error("{0}")]
    FailPermanently(String),
    #[error("{0}")]
    TryAgainLater(String),
}

pub trait Job<ExtraConfig = ()>: Send + Sync {
    type Arguments: DeserializeOwned + Send + Sync;

    fn execute(
        app: &App<ExtraConfig>,
        arguments: Self::Arguments,
    ) -> impl Future<Output = Result<(), JobError>> + Send;

    fn name() -> &'static str;
}
