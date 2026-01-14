use std::future::Future;
use std::pin::Pin;
use std::{collections::HashMap, sync::Arc};

use crate::app::App;

use super::{job_result::JobResult, Job, JobError};

/// Type alias for job executor function to reduce type complexity
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type JobExecutor =
    Arc<dyn Fn(&App, serde_json::Value) -> BoxFuture<'static, Result<(), JobError>> + Send + Sync>;

#[derive(Clone)]
pub struct JobRegistry {
    jobs: HashMap<&'static str, JobExecutor>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
        }
    }

    pub fn register_job<J: Job + 'static>(&mut self) {
        self.jobs.insert(
            J::name(),
            Arc::new(|app: &App, args_json: serde_json::Value| {
                let app = app.clone();
                Box::pin(async move {
                    let arguments: J::Arguments =
                        serde_json::from_value(args_json).map_err(|e| {
                            JobError::FailPermanently(format!("Failed to parse job arguments: {e}"))
                        })?;
                    J::execute(&app, arguments).await
                })
            }),
        );
    }

    pub(crate) fn job_names(&self) -> impl Iterator<Item = &&'static str> {
        self.jobs.keys()
    }

    pub(crate) async fn execute(
        &self,
        app: &App,
        r#type: &str,
        arguments: &serde_json::Value,
    ) -> super::job_result::JobResult {
        if let Some(executor) = self.jobs.get(r#type) {
            match executor(app, arguments.clone()).await {
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

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}
