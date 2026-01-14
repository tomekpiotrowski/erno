mod advisory_lock;
pub mod job_registry;
pub mod job_result;
pub mod job_supervisor;
pub mod scheduled_job;
mod scheduler;
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

pub trait Job: Send + Sync {
    type Arguments: DeserializeOwned + Send + Sync;

    fn execute(
        app: &App,
        arguments: Self::Arguments,
    ) -> impl Future<Output = Result<(), JobError>> + Send;

    fn name() -> &'static str;
}
