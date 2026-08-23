//! Docs: docs/src/content/docs/api/jobs.md
// Public so other framework subsystems can take a deployment-wide singleton
// lock — the monitoring collector's retention sweep does exactly that.
pub mod advisory_lock;
pub mod failure_handler;
pub mod job_registry;
pub mod job_result;
pub mod job_supervisor;
pub mod scheduled_job;
mod scheduler;
pub mod send_already_registered_email_job;
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

    // --- Optional per-job retry/failure overrides ---
    //
    // Each returns `None` by default, meaning "inherit". Resolution precedence
    // is: per-job override → worker-pool override → `jobs.defaults`.

    /// Override the maximum number of retry attempts for this job.
    fn max_retries() -> Option<i32> {
        None
    }

    /// Override the base delay (seconds) before the first retry for this job.
    fn base_retry_delay_seconds() -> Option<u64> {
        None
    }

    /// Override the exponential backoff multiplier for this job.
    fn retry_backoff_multiplier() -> Option<u64> {
        None
    }

    /// Override the execution timeout (seconds) for this job.
    fn timeout_seconds() -> Option<u32> {
        None
    }

    /// Called once when this job has permanently failed (failed permanently or
    /// exhausted its retries). Default is a no-op; override for job-specific
    /// alerting or compensating actions. The app-wide
    /// [`JobFailureHandler`](crate::jobs::failure_handler::JobFailureHandler),
    /// if registered, runs in addition to this.
    ///
    /// `arguments` is the raw job payload (it may not deserialize if that was the
    /// cause of failure); `error` is the failure message.
    fn on_permanent_failure(
        app: &App<ExtraConfig>,
        arguments: &serde_json::Value,
        error: &str,
    ) -> impl Future<Output = ()> + Send {
        let _ = (app, arguments, error);
        async {}
    }
}
