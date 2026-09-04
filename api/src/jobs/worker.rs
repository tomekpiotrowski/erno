use chrono::NaiveDateTime;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, TransactionTrait,
};
use sqlx::postgres::PgListener;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{debug, error, info, warn, Instrument};

use crate::app::App;
use crate::{
    database::models::{
        job::{self, Entity as JobEntity},
        job_execution,
        job_result::JobResult as JobResultEnum,
        job_status::JobStatus,
    },
    {
        config::{ResolvedRetryConfig, WorkerQueueConfig},
        jobs::{job_result::JobResult, JobError},
    },
};

use super::job_registry::{JobRegistry, JobRetryOverrides};

const POLL_INTERVAL_SECS: u64 = 30;

pub async fn worker<ExtraConfig>(
    worker_instance_name: &str,
    worker_config: &WorkerQueueConfig,
    app: App<ExtraConfig>,
    job_registry: &JobRegistry<ExtraConfig>,
    shutdown: &crate::shutdown::Shutdown,
) -> Result<(), DbErr>
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let sqlx_pool = app.db.get_postgres_connection_pool();
    let mut listener = PgListener::connect_with(sqlx_pool)
        .await
        .map_err(|e| DbErr::Custom(e.to_string()))?;
    listener
        .listen("job_new")
        .await
        .map_err(|e| DbErr::Custom(e.to_string()))?;
    info!(
        "Worker '{}' listening for instant job notifications",
        worker_instance_name
    );

    loop {
        // Stop claiming new work once shutdown starts. Checked here rather than
        // mid-job on purpose: a job interrupted after being claimed is left in
        // `running` and stays invisible until the stuck-job sweeper reclaims
        // it, so finishing the one in hand is always better than abandoning it.
        if shutdown.is_shutting_down() {
            info!(
                "Worker '{}' stopping: no new jobs will be claimed",
                worker_instance_name
            );
            return Ok(());
        }

        // Try to claim and execute all available jobs (drain the queue)
        let mut jobs_processed = 0;
        loop {
            if shutdown.is_shutting_down() {
                return Ok(());
            }
            let job_option = claim_oldest_viable_job(worker_config, &app.db).await?;

            let Some(job) = job_option else {
                // No more jobs available
                if jobs_processed > 0 {
                    debug!(
                        "Worker '{}' processed {} job(s), queue drained",
                        worker_instance_name, jobs_processed
                    );
                }
                break;
            };

            debug!(
                "🔧 Worker '{worker_instance_name}' claimed {status} {1}({0})",
                job.id,
                job.r#type,
                status = job.status,
            );

            // Execute the job
            execute_and_update_job(
                &job,
                worker_config,
                &app,
                job_registry,
                worker_instance_name,
            )
            .await?;

            jobs_processed += 1;
        }

        // Wait for NOTIFY or periodic timeout as a safety net
        match timeout(Duration::from_secs(POLL_INTERVAL_SECS), listener.recv()).await {
            Ok(Ok(_)) => {
                debug!(
                    "Worker '{}' received job notification",
                    worker_instance_name
                );
            }
            Ok(Err(e)) => {
                error!("Worker '{}' PgListener error: {}", worker_instance_name, e);
                return Err(DbErr::Custom(e.to_string()));
            }
            Err(_) => {
                debug!(
                    "Worker '{}' polling (no notifications for {}s)",
                    worker_instance_name, POLL_INTERVAL_SECS
                );
            }
        }
    }
}

async fn execute_and_update_job<ExtraConfig>(
    job_model: &job::Model,
    worker_config: &WorkerQueueConfig,
    app: &App<ExtraConfig>,
    job_registry: &JobRegistry<ExtraConfig>,
    worker_instance_name: &str,
) -> Result<(), DbErr>
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    // Resolve effective retry/timeout settings: per-job override → worker-pool
    // override → app-wide defaults.
    let resolved = resolve_retry_config(
        &app.config.jobs.defaults,
        worker_config,
        job_registry.retry_overrides(&job_model.r#type),
    );

    // How long the job sat before a worker picked it up.
    //
    // Measured from when it *became runnable*, not from when it was created: a
    // job scheduled for later, or backing off after a failure, is not waiting
    // until its time arrives. Counting that as latency would make a healthy
    // retry queue look permanently late.
    //
    // This, rather than queue depth, is what predicts user-visible lateness —
    // a thousand jobs enqueued a second ago is fine; one job waiting ten
    // minutes is not.
    let runnable_at = job_model
        .next_execution_at
        .unwrap_or(job_model.created_at)
        .max(job_model.created_at);
    let queue_wait = (chrono::Utc::now().naive_utc() - runnable_at)
        .num_milliseconds()
        .max(0) as f64
        / 1000.0;
    metrics::histogram!("erno_jobs_queue_wait_seconds",
        "job_type" => job_model.r#type.clone(),
    )
    .record(queue_wait);

    let otel_name = format!("job.{}", job_model.r#type);
    let span = tracing::info_span!(
        "job.execute",
        otel.name = otel_name.as_str(),
        job_type = job_model.r#type.as_str(),
        job.id = %job_model.id,
        queue_wait,
        otel.status_code = tracing::field::Empty,
    );

    // Execute the job and measure execution time
    let start_time = Instant::now();
    let timeout_duration = Duration::from_secs(u64::from(resolved.job_timeout));

    let result = (timeout(timeout_duration, async {
        job_registry
            .execute(app, &job_model.r#type, &job_model.arguments)
            .await
    })
    .instrument(span.clone())
    .await)
        .unwrap_or(JobResult::TimedOut);

    let execution_duration = start_time.elapsed();

    // Record job execution metrics
    let result_label = match &result {
        JobResult::Completed => "completed",
        JobResult::Failed(_) => "failed",
        JobResult::TimedOut => "timed_out",
    };
    span.record(
        "otel.status_code",
        if matches!(result, JobResult::Completed) {
            "OK"
        } else {
            "ERROR"
        },
    );
    metrics::counter!("jobs_executed_total",
        "job_type" => job_model.r#type.clone(),
        "result" => result_label,
    )
    .increment(1);
    metrics::histogram!("jobs_execution_duration_seconds",
        "job_type" => job_model.r#type.clone(),
    )
    .record(execution_duration.as_secs_f64());

    // Update job status based on result
    let permanently_failed = update_job_after_execution(
        job_model,
        &result,
        execution_duration,
        &resolved,
        &app.db,
        worker_instance_name,
    )
    .await?;

    // On permanent failure, run the per-job hook and the app-wide failure
    // handler (in addition to the error log emitted above).
    if permanently_failed {
        let error_msg = match &result {
            JobResult::Failed(e) => e.to_string(),
            JobResult::TimedOut => "Job execution timed out".to_string(),
            JobResult::Completed => String::new(),
        };
        job_registry
            .run_permanent_failure_hook(app, &job_model.r#type, &job_model.arguments, &error_msg)
            .await;
        if let Some(handler) = &app.job_failure_handler {
            handler
                .on_permanent_failure(&job_model.r#type, &job_model.arguments, &error_msg)
                .await;
        }
    }

    Ok(())
}

/// Merge per-job overrides, worker-pool overrides, and app-wide defaults into
/// the effective settings for a single execution. Precedence: job → pool → default.
fn resolve_retry_config(
    defaults: &crate::config::JobRetryDefaults,
    worker: &WorkerQueueConfig,
    job: JobRetryOverrides,
) -> ResolvedRetryConfig {
    ResolvedRetryConfig {
        job_timeout: job
            .job_timeout
            .or(worker.job_timeout)
            .unwrap_or(defaults.job_timeout),
        max_retries: job
            .max_retries
            .or(worker.max_retries)
            .unwrap_or(defaults.max_retries),
        base_retry_delay_seconds: job
            .base_retry_delay_seconds
            .or(worker.base_retry_delay_seconds)
            .unwrap_or(defaults.base_retry_delay_seconds),
        retry_backoff_multiplier: job
            .retry_backoff_multiplier
            .or(worker.retry_backoff_multiplier)
            .unwrap_or(defaults.retry_backoff_multiplier),
    }
}

async fn claim_oldest_viable_job(
    worker_config: &WorkerQueueConfig,
    db: &DatabaseConnection,
) -> Result<Option<job::Model>, DbErr> {
    let txn = db.begin().await?;
    let now = chrono::Utc::now().naive_utc();

    // Query for all viable jobs (pending jobs that are ready for execution).
    // Jobs that exhaust their retries are marked `Failed` (terminal) by the
    // failure handler, so the status filter alone keeps them from being
    // re-claimed — no need to filter on a fixed `max_retries` here, which would
    // be wrong once per-job overrides allow a higher limit than the pool's.
    let job_option = JobEntity::find()
        .filter(job::Column::Type.is_in(worker_config.jobs.iter()))
        .filter(job::Column::Status.is_in([JobStatus::Pending, JobStatus::PendingRetry]))
        .filter(
            job::Column::NextExecutionAt
                .is_null()
                .or(job::Column::NextExecutionAt.lte(now)),
        )
        .order_by_asc(job::Column::CreatedAt) // Select oldest job first
        .limit(1)
        .lock_exclusive()
        .one(&txn)
        .await?;

    let Some(job_model) = job_option else {
        txn.commit().await?;
        return Ok(None);
    };

    // Mark job as running
    let mut active_model: job::ActiveModel = job_model.clone().into();
    active_model.status = sea_orm::Set(JobStatus::Running);
    active_model.update(&txn).await?;

    txn.commit().await?;
    Ok(Some(job_model))
}

/// Records the execution attempt and updates job status. Returns `true` if the
/// job permanently failed on this attempt (exhausted retries or failed
/// permanently), so the caller can run failure hooks.
async fn update_job_after_execution(
    job_model: &job::Model,
    execution_result: &JobResult,
    execution_duration: Duration,
    resolved: &ResolvedRetryConfig,
    db: &DatabaseConnection,
    worker_instance_name: &str,
) -> Result<bool, DbErr> {
    let now = chrono::Utc::now().naive_utc();
    #[allow(clippy::cast_possible_truncation)]
    let execution_time_ms = execution_duration.as_millis() as i64;

    // Create JobExecution record for this execution attempt
    let job_execution_active_model = job_execution::ActiveModel {
        id: sea_orm::Set(uuid::Uuid::new_v4()),
        job_id: sea_orm::Set(job_model.id),
        result: sea_orm::Set(match execution_result {
            JobResult::Completed => JobResultEnum::Completed,
            JobResult::Failed(_) => JobResultEnum::Failed,
            JobResult::TimedOut => JobResultEnum::TimedOut,
        }),
        started_at: sea_orm::Set(now - chrono::Duration::milliseconds(execution_time_ms)),
        finished_at: sea_orm::Set(now),
        execution_time_ms: sea_orm::Set(execution_time_ms),
        failure_reason: sea_orm::Set(match execution_result {
            JobResult::Failed(reason) => Some(reason.to_string()),
            JobResult::TimedOut => Some("Job execution timed out".to_string()),
            JobResult::Completed => None,
        }),
        created_at: sea_orm::Set(now),
    };

    job_execution_active_model.insert(db).await?;

    match execution_result {
        JobResult::Completed => {
            // Job succeeded - mark as completed
            info!(
                "✅ Worker '{worker_instance_name}' completed job {}({}) created at {} in {:?}",
                job_model.r#type, job_model.id, job_model.created_at, execution_duration
            );
            let mut active_job: job::ActiveModel = job_model.clone().into();
            active_job.status = sea_orm::Set(JobStatus::Completed);
            active_job.update(db).await?;
            Ok(false)
        }
        result => {
            // Job failed - handle retry logic
            let current_retry_count = job_model.retry_count;
            handle_job_failure(
                job_model,
                result,
                current_retry_count,
                resolved,
                db,
                worker_instance_name,
                execution_duration,
            )
            .await
        }
    }
}

/// Schedules a retry or marks the job permanently failed. Returns `true` when the
/// job was marked permanently failed.
async fn handle_job_failure(
    job_model: &job::Model,
    result: &JobResult,
    current_retry_count: i32,
    resolved: &ResolvedRetryConfig,
    db: &DatabaseConnection,
    worker_instance_name: &str,
    execution_duration: Duration,
) -> Result<bool, DbErr> {
    let should_retry = match result {
        JobResult::Failed(JobError::FailPermanently(_)) => false,
        JobResult::Failed(JobError::TryAgainLater(_)) | JobResult::TimedOut => {
            current_retry_count < resolved.max_retries
        }
        JobResult::Completed => false,
    };

    if should_retry {
        let msg = match result {
            JobResult::Failed(e) => format!("{e}"),
            JobResult::TimedOut => "Timed out".to_string(),
            _ => "Unknown error".to_string(),
        };
        warn!(
            "⚠️ Worker '{worker_instance_name}' retrying job {}({}) after {:?}: {}",
            job_model.r#type, job_model.id, execution_duration, msg
        );

        // Schedule for retry
        let next_execution_at = calculate_next_retry_time(current_retry_count, resolved);

        update_job_for_retry(job_model, next_execution_at, current_retry_count + 1, db).await?;
        Ok(false)
    } else {
        let msg = match result {
            JobResult::Failed(e) => format!("{e}"),
            JobResult::TimedOut => "Timed out".to_string(),
            _ => "Unknown error".to_string(),
        };
        error!(
            "❌ Worker '{worker_instance_name}' failed job {}({}) in {:?}: {}",
            job_model.r#type, job_model.id, execution_duration, msg
        );

        update_job_as_permanently_failed(job_model, result, db).await?;
        Ok(true)
    }
}

async fn update_job_for_retry(
    job_model: &job::Model,
    next_execution_at: NaiveDateTime,
    retry_count: i32,
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    let mut active_model: job::ActiveModel = job_model.clone().into();
    active_model.status = sea_orm::Set(JobStatus::PendingRetry);
    active_model.retry_count = sea_orm::Set(retry_count);
    active_model.next_execution_at = sea_orm::Set(Some(next_execution_at));
    active_model.update(db).await?;
    Ok(())
}

async fn update_job_as_permanently_failed(
    job_model: &job::Model,
    result: &JobResult,
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    let mut active_model: job::ActiveModel = job_model.clone().into();
    active_model.status = sea_orm::Set(match result {
        JobResult::Failed(_) | JobResult::TimedOut => JobStatus::Failed,
        JobResult::Completed => JobStatus::Completed, // Should not happen in this context
    });
    active_model.update(db).await?;
    Ok(())
}

fn calculate_next_retry_time(retry_count: i32, resolved: &ResolvedRetryConfig) -> NaiveDateTime {
    let delay_seconds = resolved.base_retry_delay_seconds
        * resolved
            .retry_backoff_multiplier
            .pow(retry_count.try_into().unwrap_or(5));

    let delay_seconds_i64 = delay_seconds.try_into().unwrap_or(i64::MAX);
    chrono::Utc::now().naive_utc() + chrono::Duration::seconds(delay_seconds_i64)
}

// Execution is provided by the application via the `executor` function parameter.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::JobRetryDefaults;

    fn defaults() -> JobRetryDefaults {
        JobRetryDefaults {
            job_timeout: 300,
            max_retries: 4,
            base_retry_delay_seconds: 60,
            retry_backoff_multiplier: 5,
        }
    }

    fn pool(
        job_timeout: Option<u32>,
        max_retries: Option<i32>,
        base_retry_delay_seconds: Option<u64>,
        retry_backoff_multiplier: Option<u64>,
    ) -> WorkerQueueConfig {
        WorkerQueueConfig {
            jobs: vec![],
            count: 1,
            job_timeout,
            max_retries,
            base_retry_delay_seconds,
            retry_backoff_multiplier,
        }
    }

    #[test]
    fn resolve_uses_app_defaults_when_nothing_overrides() {
        let resolved = resolve_retry_config(
            &defaults(),
            &pool(None, None, None, None),
            JobRetryOverrides::default(),
        );
        assert_eq!(resolved.max_retries, 4);
        assert_eq!(resolved.job_timeout, 300);
        assert_eq!(resolved.base_retry_delay_seconds, 60);
        assert_eq!(resolved.retry_backoff_multiplier, 5);
    }

    #[test]
    fn pool_overrides_defaults_per_field() {
        let resolved = resolve_retry_config(
            &defaults(),
            &pool(None, Some(8), Some(30), None),
            JobRetryOverrides::default(),
        );
        // Pool-set fields win; unset fields inherit app defaults.
        assert_eq!(resolved.max_retries, 8);
        assert_eq!(resolved.base_retry_delay_seconds, 30);
        assert_eq!(resolved.retry_backoff_multiplier, 5);
        assert_eq!(resolved.job_timeout, 300);
    }

    #[test]
    fn job_override_wins_over_pool_and_default() {
        let job = JobRetryOverrides {
            max_retries: Some(10),
            base_retry_delay_seconds: None,
            retry_backoff_multiplier: None,
            job_timeout: Some(5),
        };
        let resolved = resolve_retry_config(&defaults(), &pool(None, Some(8), Some(30), None), job);
        // Precedence: job → pool → default.
        assert_eq!(resolved.max_retries, 10); // job beats pool's 8
        assert_eq!(resolved.job_timeout, 5); // job beats default 300
        assert_eq!(resolved.base_retry_delay_seconds, 30); // pool, since job unset
        assert_eq!(resolved.retry_backoff_multiplier, 5); // default, since both unset
    }

    #[test]
    fn next_retry_time_uses_resolved_backoff() {
        let resolved = ResolvedRetryConfig {
            job_timeout: 300,
            max_retries: 5,
            base_retry_delay_seconds: 10,
            retry_backoff_multiplier: 2,
        };
        let now = chrono::Utc::now().naive_utc();
        // delay = base * multiplier^retry_count
        let t0 = calculate_next_retry_time(0, &resolved); // 10 * 2^0 = 10s
        let t2 = calculate_next_retry_time(2, &resolved); // 10 * 2^2 = 40s
        let d0 = (t0 - now).num_seconds();
        let d2 = (t2 - now).num_seconds();
        assert!((9..=12).contains(&d0), "expected ~10s, got {d0}");
        assert!((39..=42).contains(&d2), "expected ~40s, got {d2}");
    }

    // --- DB-backed tests for the worker's failure/retry path ---

    use crate::jobs::{Job, JobError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn no_router(_app: crate::app::App) -> axum::Router {
        axum::Router::new()
    }

    fn no_fixtures(
        db: &sea_orm::DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let _ = db;
        })
    }

    fn test_app(
        t: &crate::tests::setup_test::TestUtils,
        handler: Option<Arc<dyn crate::jobs::failure_handler::JobFailureHandler>>,
    ) -> crate::app::App {
        crate::app::App {
            shutdown: crate::shutdown::Shutdown::never(),
            config: t.config.clone(),
            environment: t.environment,
            db: t.db.clone(),
            mailer: t.mailer.clone(),
            job_queue: t.job_queue.clone(),
            sync_queue: crate::sync::queue::SyncQueue::mock(),
            sync_registry: Arc::new(crate::sync::registry::SyncRegistry::new()),
            rate_limit_state: crate::rate_limiting::RateLimitState::new(
                t.config.rate_limiting.clone(),
            ),
            websocket_connections: crate::websocket::connections::Connections::new(),
            storage: crate::storage::FileStorage::mock(),
            prometheus_handle: crate::metrics::setup_metrics(),
            metrics_collectors: Arc::new(crate::metrics::collector::CollectorRegistry::default()),
            job_failure_handler: handler,
            user_data_deleter: None,
            error_reporter: crate::error_reporting::reporter::ErrorReporter::disabled(),
            skip_default_cors: false,
        }
    }

    async fn insert_running_job(db: &sea_orm::DatabaseConnection, job_type: &str) -> job::Model {
        use sea_orm::{ActiveModelTrait, Set};
        job::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            r#type: Set(job_type.to_string()),
            arguments: Set(serde_json::json!({})),
            status: Set(JobStatus::Running),
            retry_count: Set(0),
            next_execution_at: Set(None),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    fn pool_for(job_type: &str) -> WorkerQueueConfig {
        WorkerQueueConfig {
            jobs: vec![job_type.to_string()],
            count: 1,
            job_timeout: None,
            max_retries: None,
            base_retry_delay_seconds: None,
            retry_backoff_multiplier: None,
        }
    }

    #[tokio::test]
    async fn permanent_failure_runs_per_job_and_app_wide_hooks() {
        use sea_orm::EntityTrait;

        // A job whose per-job override forbids retries, so a TryAgainLater
        // failure becomes permanent on the first attempt.
        static PER_JOB_CALLS: AtomicUsize = AtomicUsize::new(0);
        struct FailingJob;
        impl Job for FailingJob {
            type Arguments = serde_json::Value;
            fn name() -> &'static str {
                "failing_job_test"
            }
            async fn execute(
                _app: &crate::app::App,
                _args: serde_json::Value,
            ) -> Result<(), JobError> {
                Err(JobError::TryAgainLater("boom".to_string()))
            }
            fn max_retries() -> Option<i32> {
                Some(0)
            }
            async fn on_permanent_failure(
                _app: &crate::app::App,
                _args: &serde_json::Value,
                _error: &str,
            ) {
                PER_JOB_CALLS.fetch_add(1, Ordering::SeqCst);
            }
        }

        struct RecordingHandler(Arc<AtomicUsize>);
        #[async_trait::async_trait]
        impl crate::jobs::failure_handler::JobFailureHandler for RecordingHandler {
            async fn on_permanent_failure(
                &self,
                _job_type: &str,
                _arguments: &serde_json::Value,
                _error: &str,
            ) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let t = crate::tests::setup_test::setup_test::<crate::database::migrations::Migrator, _>(
            crate::tests::setup_test::test_boot(no_router),
            no_fixtures,
        )
        .await;

        let app_calls = Arc::new(AtomicUsize::new(0));
        let app = test_app(&t, Some(Arc::new(RecordingHandler(app_calls.clone()))));

        let mut registry = JobRegistry::<()>::new();
        registry.register_job::<FailingJob>();

        let job_row = insert_running_job(&t.db, "failing_job_test").await;
        execute_and_update_job(
            &job_row,
            &pool_for("failing_job_test"),
            &app,
            &registry,
            "test-0",
        )
        .await
        .unwrap();

        let updated = job::Entity::find_by_id(job_row.id)
            .one(&t.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, JobStatus::Failed);
        assert_eq!(PER_JOB_CALLS.load(Ordering::SeqCst), 1, "per-job hook");
        assert_eq!(app_calls.load(Ordering::SeqCst), 1, "app-wide hook");
    }

    #[tokio::test]
    async fn transient_failure_schedules_retry_without_hooks() {
        use sea_orm::EntityTrait;

        // No per-job override: max_retries inherits the app default (>0), so a
        // TryAgainLater failure is rescheduled rather than failed.
        static HOOK_CALLS: AtomicUsize = AtomicUsize::new(0);
        struct RetryingJob;
        impl Job for RetryingJob {
            type Arguments = serde_json::Value;
            fn name() -> &'static str {
                "retrying_job_test"
            }
            async fn execute(
                _app: &crate::app::App,
                _args: serde_json::Value,
            ) -> Result<(), JobError> {
                Err(JobError::TryAgainLater("later".to_string()))
            }
            async fn on_permanent_failure(
                _app: &crate::app::App,
                _args: &serde_json::Value,
                _error: &str,
            ) {
                HOOK_CALLS.fetch_add(1, Ordering::SeqCst);
            }
        }

        let t = crate::tests::setup_test::setup_test::<crate::database::migrations::Migrator, _>(
            crate::tests::setup_test::test_boot(no_router),
            no_fixtures,
        )
        .await;
        let app = test_app(&t, None);

        let mut registry = JobRegistry::<()>::new();
        registry.register_job::<RetryingJob>();

        let job_row = insert_running_job(&t.db, "retrying_job_test").await;
        execute_and_update_job(
            &job_row,
            &pool_for("retrying_job_test"),
            &app,
            &registry,
            "test-0",
        )
        .await
        .unwrap();

        let updated = job::Entity::find_by_id(job_row.id)
            .one(&t.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, JobStatus::PendingRetry);
        assert_eq!(updated.retry_count, 1);
        assert!(updated.next_execution_at.is_some());
        assert_eq!(
            HOOK_CALLS.load(Ordering::SeqCst),
            0,
            "no permanent-failure hook on retry"
        );
    }
}
