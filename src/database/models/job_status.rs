use sea_orm::DeriveActiveEnum;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

/// Represents the current execution state of a background job.
///
/// This enum tracks a job's lifecycle from creation through completion or failure.
/// Jobs transition through states as they are picked up by workers, executed, and
/// finalized. The status determines whether a job is eligible for execution and
/// whether it has reached a terminal state.
///
/// # State Transitions
///
/// Typical lifecycle:
/// - `Pending` → `Running` → `Completed` (success)
/// - `Pending` → `Running` → `Failed` (permanent failure or timeout)
/// - `Pending` → `Running` → `PendingRetry` (retry after transient failure)
/// - `PendingRetry` → `Running` → `Completed`/`Failed`/`PendingRetry` (subsequent attempts)
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    DeriveActiveEnum,
    Serialize,
    Deserialize,
    EnumIter,
    EnumString,
    Display,
)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "job_status")]
#[derive(Default)]
pub enum JobStatus {
    /// Job is waiting to be picked up by a worker for the first time.
    ///
    /// Jobs in this state are eligible for execution if their `next_execution_at`
    /// timestamp has been reached. This is the default state for newly created jobs.
    /// After a transient failure, jobs move to `PendingRetry` instead of back to this state.
    #[sea_orm(string_value = "pending")]
    #[default]
    Pending,

    /// Job is waiting to be retried after a previous execution failed.
    ///
    /// Jobs enter this state after encountering a transient failure (e.g., network
    /// timeout, temporary service unavailability) that may be resolved by retrying.
    /// This state helps distinguish between fresh jobs and those being retried,
    /// which is useful for monitoring and debugging.
    #[sea_orm(string_value = "pending_retry")]
    PendingRetry,

    /// Job is currently being executed by a worker.
    ///
    /// A job enters this state when a worker claims it for execution. If a worker
    /// crashes while processing a job, cleanup mechanisms should eventually transition
    /// it back to `Pending` or mark it as `Failed`.
    #[sea_orm(string_value = "running")]
    Running,

    /// Job finished successfully.
    ///
    /// This is a terminal state indicating the job's work completed without errors.
    /// Jobs in this state will not be processed again.
    #[sea_orm(string_value = "completed")]
    Completed,

    /// Job failed permanently and will not be retried.
    ///
    /// This is a terminal state for jobs that encountered non-recoverable errors
    /// (e.g., invalid input, missing required data), exceeded the execution time limit,
    /// or exhausted their retry attempts. Jobs reach this state either through explicit
    /// permanent failure, timeout, or after exceeding the maximum retry count.
    #[sea_orm(string_value = "failed")]
    Failed,
}

#[allow(dead_code)]
impl JobStatus {
    /// Checks if this status represents a terminal state.
    ///
    /// Terminal states are final - jobs in these states will not be processed again.
    /// This includes `Completed` and `Failed` (which covers timeouts as well).
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }

    /// Checks if this job is currently being executed by a worker.
    pub const fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Checks if this job is waiting to be picked up by a worker.
    ///
    /// Returns true for both `Pending` (first-time) and `PendingRetry` (retry) states.
    pub const fn is_pending(&self) -> bool {
        matches!(self, Self::Pending | Self::PendingRetry)
    }
}
