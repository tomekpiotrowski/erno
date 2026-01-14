use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter,
    QueryOrder as _, QuerySelect as _,
};
use std::{collections::HashSet, time::Duration};
use tokio::{spawn, time::sleep};
use tracing::{debug, error, info, warn};

use crate::{
    app::App,
    config::{CleanupConfig, JobsConfig, WorkerQueueConfig, WorkersConfig},
    database::models::{
        job::{self, Entity as JobEntity},
        job_execution,
        job_result::JobResult as JobResultEnum,
        job_status::JobStatus,
    },
    jobs::{
        advisory_lock::{self, lock_keys},
        scheduler::Scheduler,
        worker::worker,
    },
};

use super::{job_registry::JobRegistry, scheduled_job::ScheduledJob};

/// Verify that all job types have at least one worker pool configured to handle them.
///
/// This function ensures that every variant of the `JobType` enum has a corresponding
/// worker pool configured in the `WorkersConfig`. This is critical for preventing
/// jobs from being permanently stuck in the queue due to missing worker coverage.
///
/// # Arguments
/// * `workers_config` - The workers configuration mapping pool names to queue configurations
///
/// # Panics
/// Panics if any job type lacks worker coverage, as this is a critical configuration error
/// that would prevent the job system from functioning correctly.
fn verify_job_types_have_workers(workers_config: &WorkersConfig, job_registry: &JobRegistry) {
    // Collect all job types that have worker pools configured
    let mut covered_job_types: HashSet<&str> = HashSet::new();

    for queue_config in workers_config.workers.values() {
        for job_type_str in &queue_config.jobs {
            covered_job_types.insert(job_type_str.as_str());
        }
    }

    for job_type in job_registry.job_names() {
        assert!(
            covered_job_types.contains(*job_type),
            "No worker pool configured to handle job type '{job_type}'. Please add a worker pool for this job type."
        );
    }
}

pub async fn job_supervisor(
    jobs_config: JobsConfig,
    app: App,
    job_registry: JobRegistry,
    job_schedule: Vec<ScheduledJob>,
) {
    // Verify that all JobTypes have corresponding worker pools
    verify_job_types_have_workers(&jobs_config.workers, &job_registry);
    // Start all worker pools
    start_worker_pools(&jobs_config.workers, &app, &job_registry);

    // Start the scheduler
    start_scheduler(&app.db, job_schedule);

    // Start the stuck job recovery task
    start_recovery_task(&jobs_config.workers, &app.db);

    // Start the job cleanup task
    start_cleanup_task(&jobs_config.cleanup, &app.db);

    // Keep the supervisor running
    run_supervisor_loop().await;
}

/// Start all worker pools based on configuration
fn start_worker_pools(config: &WorkersConfig, app: &App, job_registry: &JobRegistry) {
    info!("🚀 Starting job workers");

    for (worker_name, worker_config) in &config.workers {
        info!(
            "⚡ Pool '{}': {} workers for jobs {:?}",
            worker_name, worker_config.count, worker_config.jobs
        );

        start_worker_pool(worker_name, worker_config, app, job_registry);
    }
}

/// Start a single worker pool with multiple worker instances
fn start_worker_pool(
    worker_name: &str,
    worker_config: &WorkerQueueConfig,
    app: &App,
    job_registry: &JobRegistry,
) {
    for worker_id in 0..worker_config.count {
        let worker_instance_name = format!("{worker_name}-{worker_id}");
        let worker_config_clone = worker_config.clone();
        let app_clone = app.clone();
        let job_registry_clone = job_registry.clone();

        spawn(async move {
            run_worker_with_restart(
                &worker_instance_name,
                &worker_config_clone,
                app_clone,
                job_registry_clone,
            )
            .await;
        });
    }
}

/// Run a worker with automatic restart on crash
async fn run_worker_with_restart(
    worker_instance_name: &str,
    worker_config: &WorkerQueueConfig,
    app: App,
    job_registry: JobRegistry,
) {
    let mut restart_count = 0;
    loop {
        debug!(
            "Starting worker '{}' for job types: {:?} (restart #{})",
            worker_instance_name, worker_config.jobs, restart_count
        );

        let worker_app = app.clone();
        if let Err(e) = worker(
            worker_instance_name,
            worker_config,
            worker_app,
            &job_registry,
        )
        .await
        {
            error!(
                "💥 Worker '{}' crashed (restart #{}) - error: {}",
                worker_instance_name, restart_count, e
            );
        }

        restart_count += 1;

        sleep(Duration::from_secs(10)).await;
    }
}

/// Start the job scheduler
fn start_scheduler(db: &DatabaseConnection, job_schedule: Vec<ScheduledJob>) {
    let scheduler_db = db.clone();
    let job_schedule_for_spawn = job_schedule.clone();

    spawn(async move {
        let job_schedule_inner = job_schedule_for_spawn;
        advisory_lock::run_with_advisory_lock(
            scheduler_db,
            lock_keys::SCHEDULER,
            "scheduler",
            move |db| {
                let job_schedule_clone = job_schedule_inner.clone();
                async move {
                    info!("📅 Starting job scheduler");
                    let mut scheduler = Scheduler::new(db, job_schedule_clone);
                    scheduler.run().await;
                }
            },
        )
        .await;
    });
}

/// Start the stuck job recovery task
fn start_recovery_task(config: &WorkersConfig, db: &DatabaseConnection) {
    let recovery_config = config.clone();
    let recovery_db = db.clone();
    spawn(async move {
        advisory_lock::run_with_advisory_lock(
            recovery_db,
            lock_keys::RECOVERY,
            "stuck job recovery",
            move |db| {
                info!("🏥 Starting stuck job recovery");
                let config = recovery_config.clone();
                async move {
                    run_recovery_loop(&config, &db).await;
                }
            },
        )
        .await;
    });
}

/// Keep the supervisor running indefinitely
async fn run_supervisor_loop() {
    loop {
        sleep(Duration::from_secs(3600)).await;
    }
}

async fn run_recovery_loop(config: &WorkersConfig, db: &DatabaseConnection) {
    loop {
        match recover_stuck_jobs(config, db).await {
            Ok(recovered_count) => {
                if recovered_count > 0 {
                    info!("🏥 Recovered {} stuck jobs", recovered_count);
                }
            }
            Err(e) => {
                error!("❌ Failed to recover stuck jobs: {}", e);
                break; // Break inner loop on error to trigger restart
            }
        }

        // Check for stuck jobs every 5 minutes
        sleep(Duration::from_secs(300)).await;
    }
}

/// Finds and recovers jobs that have been running longer than 2x their timeout
async fn recover_stuck_jobs(
    config: &WorkersConfig,
    db: &DatabaseConnection,
) -> Result<usize, DbErr> {
    let mut total_recovered = 0;

    for (pool_name, worker_config) in &config.workers {
        let recovered_count = recover_stuck_jobs_for_pool(pool_name, worker_config, db).await?;
        total_recovered += recovered_count;
    }

    Ok(total_recovered)
}

async fn recover_stuck_jobs_for_pool(
    pool_name: &str,
    worker_config: &WorkerQueueConfig,
    db: &DatabaseConnection,
) -> Result<usize, DbErr> {
    // Calculate the stuck threshold: 2x the job timeout
    let stuck_threshold_seconds = worker_config.job_timeout * 2;
    let stuck_threshold = chrono::Duration::seconds(stuck_threshold_seconds.into());
    let cutoff_time = chrono::Utc::now().naive_utc() - stuck_threshold;

    // Find jobs in this pool that have been running too long
    let stuck_jobs = JobEntity::find()
        .filter(job::Column::Status.eq(JobStatus::Running))
        .filter(job::Column::Type.is_in(&worker_config.jobs))
        .filter(job::Column::UpdatedAt.lte(cutoff_time))
        .all(db)
        .await?;

    let mut recovered_count = 0;
    for stuck_job in stuck_jobs {
        recover_individual_stuck_job(stuck_job, pool_name, stuck_threshold_seconds, db).await?;
        recovered_count += 1;
    }

    Ok(recovered_count)
}

async fn recover_individual_stuck_job(
    stuck_job: job::Model,
    pool_name: &str,
    stuck_threshold_seconds: u32,
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    let running_duration = chrono::Utc::now()
        .naive_utc()
        .signed_duration_since(stuck_job.updated_at);

    warn!(
        "🏥 Recovering stuck job {}({}) in pool '{}' - running for {}s (threshold: {}s)",
        stuck_job.id,
        stuck_job.r#type,
        pool_name,
        running_duration.num_seconds(),
        stuck_threshold_seconds
    );

    // Create a JobExecution record for this failed attempt
    let now = chrono::Utc::now().naive_utc();
    let execution_time_ms = running_duration.num_milliseconds();

    let job_execution_active_model = job_execution::ActiveModel {
        id: sea_orm::Set(uuid::Uuid::new_v4()),
        job_id: sea_orm::Set(stuck_job.id),
        result: sea_orm::Set(JobResultEnum::TimedOut),
        started_at: sea_orm::Set(stuck_job.updated_at),
        finished_at: sea_orm::Set(now),
        execution_time_ms: sea_orm::Set(execution_time_ms),
        failure_reason: sea_orm::Set(Some(format!(
            "Job recovered after running for {}s (exceeded threshold of {}s)",
            running_duration.num_seconds(),
            stuck_threshold_seconds
        ))),
        created_at: sea_orm::Set(now),
    };

    job_execution_active_model.insert(db).await?;

    // Reset the job to Pending status for retry
    let mut active_job: job::ActiveModel = stuck_job.into();
    active_job.status = sea_orm::Set(JobStatus::Pending);
    active_job.update(db).await?;

    Ok(())
}

/// Start the job cleanup task
fn start_cleanup_task(config: &CleanupConfig, db: &DatabaseConnection) {
    let cleanup_config = config.clone();
    let cleanup_db = db.clone();

    spawn(async move {
        advisory_lock::run_with_advisory_lock(
            cleanup_db,
            lock_keys::CLEANUP,
            "job cleanup",
            move |db| {
                let config = cleanup_config.clone();
                async move {
                    info!("🧹 Starting job cleanup task");
                    run_cleanup_loop(&config, &db).await;
                }
            },
        )
        .await;
    });
}

async fn run_cleanup_loop(config: &CleanupConfig, db: &DatabaseConnection) {
    loop {
        if let Err(e) = cleanup_old_jobs(config, db).await {
            error!("🧹 Failed to clean up old jobs: {}", e);
        }

        // Wait for the configured interval between cleanup runs
        sleep(Duration::from_secs(config.interval_seconds)).await;
    }
}

/// Clean up old completed and failed jobs along with their executions
async fn cleanup_old_jobs(config: &CleanupConfig, db: &DatabaseConnection) -> Result<(), DbErr> {
    let now = chrono::Utc::now().naive_utc();

    // Calculate cutoff times
    let completed_cutoff = now
        - chrono::Duration::seconds(
            config
                .completed_retention_seconds
                .try_into()
                .unwrap_or(7200),
        );
    let failed_cutoff = now
        - chrono::Duration::seconds(
            config
                .failed_retention_seconds
                .try_into()
                .unwrap_or(172_800),
        );

    // Clean up completed jobs
    cleanup_jobs_by_status(
        db,
        &[JobStatus::Completed],
        completed_cutoff,
        config.batch_size,
    )
    .await?;

    // Clean up failed jobs (including timed out jobs)
    cleanup_jobs_by_status(db, &[JobStatus::Failed], failed_cutoff, config.batch_size).await?;

    Ok(())
}

/// Clean up jobs with specific statuses older than the cutoff time
async fn cleanup_jobs_by_status(
    db: &DatabaseConnection,
    statuses: &[JobStatus],
    cutoff_time: chrono::NaiveDateTime,
    batch_size: usize,
) -> Result<(), DbErr> {
    loop {
        // Find a batch of old jobs to delete
        let old_jobs = JobEntity::find()
            .filter(job::Column::Status.is_in(statuses.iter().copied()))
            .filter(job::Column::CreatedAt.lte(cutoff_time))
            .order_by_asc(job::Column::CreatedAt)
            .limit(batch_size as u64)
            .all(db)
            .await?;

        if old_jobs.is_empty() {
            break; // No more jobs to clean up
        }

        let job_ids: Vec<uuid::Uuid> = old_jobs.iter().map(|job| job.id).collect();
        let batch_count = job_ids.len();

        // Delete the jobs
        JobEntity::delete_many()
            .filter(job::Column::Id.is_in(job_ids))
            .exec(db)
            .await?;

        debug!("🧹 Deleted batch of {} old jobs", batch_count);

        // Small delay between batches to avoid overwhelming the database
        sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}
