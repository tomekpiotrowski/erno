use std::future::Future;
use std::pin::Pin;
use std::{collections::HashMap, sync::Arc};

use crate::app::App;

use super::{job_result::JobResult, Job, JobError};

/// Type alias for job executor function to reduce type complexity
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type JobExecutor<ExtraConfig> = Arc<
    dyn Fn(&App<ExtraConfig>, serde_json::Value) -> BoxFuture<'static, Result<(), JobError>>
        + Send
        + Sync,
>;
type FailureHook<ExtraConfig> = Arc<
    dyn Fn(&App<ExtraConfig>, serde_json::Value, String) -> BoxFuture<'static, ()> + Send + Sync,
>;

/// Per-job retry/failure overrides captured from the [`Job`] trait at
/// registration time. `None` for any field means "inherit" (worker pool, then
/// `jobs.defaults`).
#[derive(Debug, Clone, Copy, Default)]
pub struct JobRetryOverrides {
    pub max_retries: Option<i32>,
    pub base_retry_delay_seconds: Option<u64>,
    pub retry_backoff_multiplier: Option<u64>,
    pub job_timeout: Option<u32>,
}

struct RegisteredJob<ExtraConfig> {
    executor: JobExecutor<ExtraConfig>,
    retry: JobRetryOverrides,
    on_permanent_failure: FailureHook<ExtraConfig>,
}

#[derive(Clone)]
pub struct JobRegistry<ExtraConfig = ()> {
    jobs: HashMap<&'static str, Arc<RegisteredJob<ExtraConfig>>>,
}

impl<ExtraConfig> JobRegistry<ExtraConfig>
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
        }
    }

    pub fn register_job<J: Job<ExtraConfig> + 'static>(&mut self) {
        let executor: JobExecutor<ExtraConfig> =
            Arc::new(|app: &App<ExtraConfig>, args_json: serde_json::Value| {
                let app = app.clone();
                Box::pin(async move {
                    let arguments: J::Arguments =
                        serde_json::from_value(args_json).map_err(|e| {
                            JobError::FailPermanently(format!("Failed to parse job arguments: {e}"))
                        })?;
                    J::execute(&app, arguments).await
                })
            });

        let on_permanent_failure: FailureHook<ExtraConfig> = Arc::new(
            |app: &App<ExtraConfig>, args_json: serde_json::Value, error: String| {
                let app = app.clone();
                Box::pin(async move {
                    J::on_permanent_failure(&app, &args_json, &error).await;
                })
            },
        );

        self.jobs.insert(
            J::name(),
            Arc::new(RegisteredJob {
                executor,
                retry: JobRetryOverrides {
                    max_retries: J::max_retries(),
                    base_retry_delay_seconds: J::base_retry_delay_seconds(),
                    retry_backoff_multiplier: J::retry_backoff_multiplier(),
                    job_timeout: J::timeout_seconds(),
                },
                on_permanent_failure,
            }),
        );
    }

    pub(crate) fn job_names(&self) -> impl Iterator<Item = &&'static str> {
        self.jobs.keys()
    }

    /// Per-job retry overrides for `type`, or all-`None` if the job is unknown.
    pub(crate) fn retry_overrides(&self, r#type: &str) -> JobRetryOverrides {
        self.jobs.get(r#type).map(|j| j.retry).unwrap_or_default()
    }

    /// Invoke the job's `on_permanent_failure` hook, if the job is registered.
    pub(crate) async fn run_permanent_failure_hook(
        &self,
        app: &App<ExtraConfig>,
        r#type: &str,
        arguments: &serde_json::Value,
        error: &str,
    ) {
        if let Some(job) = self.jobs.get(r#type) {
            (job.on_permanent_failure)(app, arguments.clone(), error.to_string()).await;
        }
    }

    pub(crate) async fn execute(
        &self,
        app: &App<ExtraConfig>,
        r#type: &str,
        arguments: &serde_json::Value,
    ) -> super::job_result::JobResult {
        if let Some(job) = self.jobs.get(r#type) {
            match (job.executor)(app, arguments.clone()).await {
                Ok(_) => JobResult::Completed,
                Err(e) => JobResult::Failed(e),
            }
        } else {
            JobResult::Failed(JobError::FailPermanently(format!(
                "No job registered for job type: {type}"
            )))
        }
    }
}

impl<ExtraConfig> Default for JobRegistry<ExtraConfig>
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}
