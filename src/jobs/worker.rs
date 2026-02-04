use chrono::NaiveDateTime;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, TransactionTrait,
};
use sqlx::postgres::PgListener;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};

use crate::app::App;
use crate::{
    database::models::{
        job::{self, Entity as JobEntity},
        job_execution,
        job_result::JobResult as JobResultEnum,
        job_status::JobStatus,
    },
    {
        config::WorkerQueueConfig,
        jobs::{job_result::JobResult, JobError},
    },
};

use super::job_registry::JobRegistry;

const FALLBACK_POLL_INTERVAL_SECS: u64 = 30;

pub async fn worker(
    worker_instance_name: &str,
    worker_config: &WorkerQueueConfig,
    app: App,
    job_registry: &JobRegistry,
) -> Result<(), DbErr> {
    // Try to set up LISTEN for instant job notifications
    let sqlx_pool = app.db.get_postgres_connection_pool();
    let mut listener = match PgListener::connect_with(sqlx_pool).await {
        Ok(mut l) => {
            if let Err(e) = l.listen("job_new").await {
                warn!(
                    "Worker '{}' failed to LISTEN on 'job_new': {}. Using polling fallback.",
                    worker_instance_name, e
                );
                None
            } else {
                info!(
                    "Worker '{}' listening for instant job notifications",
                    worker_instance_name
                );
                Some(l)
            }
        }
        Err(e) => {
            warn!(
                "Worker '{}' failed to create PgListener: {}. Using polling fallback.",
                worker_instance_name, e
            );
            None
        }
    };

    loop {
        // Try to claim and execute all available jobs (drain the queue)
        let mut jobs_processed = 0;
        loop {
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

        // No jobs available, wait for notification or timeout
        if let Some(ref mut l) = listener {
            // Wait for NOTIFY or timeout after fallback interval
            match timeout(Duration::from_secs(FALLBACK_POLL_INTERVAL_SECS), l.recv()).await {
                Ok(Ok(_notification)) => {
                    // Received notification, loop to drain queue
                    debug!(
                        "Worker '{}' received job notification",
                        worker_instance_name
                    );
                    continue;
                }
                Ok(Err(e)) => {
                    // PgListener error, fall back to polling
                    error!(
                        "Worker '{}' PgListener error: {}. Switching to polling.",
                        worker_instance_name, e
                    );
                    listener = None;
                    sleep(Duration::from_secs(1)).await;
                }
                Err(_) => {
                    // Timeout - fallback poll interval elapsed
                    debug!(
                        "Worker '{}' polling (no notifications for {}s)",
                        worker_instance_name, FALLBACK_POLL_INTERVAL_SECS
                    );
                    continue;
                }
            }
        } else {
            // No listener, use simple polling
            sleep(Duration::from_secs(1)).await;
        }
    }
}

async fn execute_and_update_job(
    job_model: &job::Model,
    worker_config: &WorkerQueueConfig,
    app: &App,
    job_registry: &JobRegistry,
    worker_instance_name: &str,
) -> Result<(), DbErr> {
    // Execute the job and measure execution time
    let start_time = Instant::now();
    let timeout_duration = Duration::from_secs(u64::from(worker_config.job_timeout));

    let result = (timeout(timeout_duration, async {
        job_registry
            .execute(app, &job_model.r#type, &job_model.arguments)
            .await
    })
    .await)
        .unwrap_or(JobResult::TimedOut);

    let execution_duration = start_time.elapsed();

    // Update job status based on result
    update_job_after_execution(
        job_model,
        &result,
        execution_duration,
        worker_config,
        &app.db,
        worker_instance_name,
    )
    .await?;

    Ok(())
}

async fn claim_oldest_viable_job(
    worker_config: &WorkerQueueConfig,
    db: &DatabaseConnection,
) -> Result<Option<job::Model>, DbErr> {
    let txn = db.begin().await?;
    let now = chrono::Utc::now().naive_utc();

    // Query for all viable jobs (pending jobs that are ready for execution)
    let job_option = JobEntity::find()
        .filter(job::Column::Type.is_in(worker_config.jobs.iter()))
        .filter(job::Column::Status.is_in([JobStatus::Pending, JobStatus::PendingRetry]))
        .filter(job::Column::RetryCount.lt(worker_config.max_retries))
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

async fn update_job_after_execution(
    job_model: &job::Model,
    execution_result: &JobResult,
    execution_duration: Duration,
    worker_config: &WorkerQueueConfig,
    db: &DatabaseConnection,
    worker_instance_name: &str,
) -> Result<(), DbErr> {
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
        }
        result => {
            // Job failed - handle retry logic
            let current_retry_count = job_model.retry_count;
            handle_job_failure(
                job_model,
                result,
                current_retry_count,
                worker_config,
                db,
                worker_instance_name,
                execution_duration,
            )
            .await?;
        }
    }

    Ok(())
}

async fn handle_job_failure(
    job_model: &job::Model,
    result: &JobResult,
    current_retry_count: i32,
    worker_config: &WorkerQueueConfig,
    db: &DatabaseConnection,
    worker_instance_name: &str,
    execution_duration: Duration,
) -> Result<(), DbErr> {
    let should_retry = match result {
        JobResult::Failed(JobError::FailPermanently(_)) => false,
        JobResult::Failed(JobError::TryAgainLater(_)) | JobResult::TimedOut => {
            current_retry_count < worker_config.max_retries
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
        let next_execution_at = calculate_next_retry_time(current_retry_count, worker_config);

        update_job_for_retry(job_model, next_execution_at, current_retry_count + 1, db).await
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

        update_job_as_permanently_failed(job_model, result, db).await
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

fn calculate_next_retry_time(retry_count: i32, worker_config: &WorkerQueueConfig) -> NaiveDateTime {
    let delay_seconds = worker_config.base_retry_delay_seconds
        * worker_config
            .retry_backoff_multiplier
            .pow(retry_count.try_into().unwrap_or(5));

    let delay_seconds_i64 = delay_seconds.try_into().unwrap_or(i64::MAX);
    chrono::Utc::now().naive_utc() + chrono::Duration::seconds(delay_seconds_i64)
}

// Execution is provided by the application via the `executor` function parameter.
