//! App-wide hook invoked when any background job permanently fails.
//!
//! Register an implementation via [`BootConfig::on_job_failure`](crate::boot::BootConfig::on_job_failure)
//! to be notified whenever a job exhausts its retries (or fails permanently),
//! regardless of job type — e.g. to send an alert email or post to an incident
//! channel. For job-specific handling, prefer overriding
//! [`Job::on_permanent_failure`](crate::jobs::Job::on_permanent_failure); both run.

use async_trait::async_trait;

/// Receives a notification when a job permanently fails.
///
/// Implementations must be cheap to clone-share (`Arc`) and safe to call from
/// the worker's failure path. Errors should be handled internally — the worker
/// does not act on this hook's outcome.
#[async_trait]
pub trait JobFailureHandler: Send + Sync {
    /// `job_type` is the registered job name, `arguments` the raw payload, and
    /// `error` the failure message.
    async fn on_permanent_failure(
        &self,
        job_type: &str,
        arguments: &serde_json::Value,
        error: &str,
    );
}
