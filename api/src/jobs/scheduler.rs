use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use std::{error::Error, str::FromStr, time::Duration};
use tokio::{
    task::JoinHandle,
    time::{sleep, sleep_until, Duration as TokioDuration, Instant},
};
use tracing::{debug, error, info};

use crate::{
    database::models::{job, job_status::JobStatus},
    jobs::scheduled_job::ScheduledJob,
};

/// Scheduler that spawns individual tasks for each scheduled job
pub struct Scheduler {
    db: DatabaseConnection,
    schedule: Vec<ScheduledJob>,
    task_handles: Vec<JoinHandle<()>>,
}

impl Scheduler {
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(db: DatabaseConnection, schedule: Vec<ScheduledJob>) -> Self {
        Self {
            db,
            schedule,
            task_handles: Vec::new(),
        }
    }

    pub async fn run(&mut self) {
        info!(
            "📅 Scheduler started with {} scheduled jobs",
            self.schedule.len()
        );

        // If there are no scheduled jobs, just wait indefinitely
        if self.schedule.is_empty() {
            debug!("📅 No scheduled jobs configured, scheduler will idle");
            // Wait indefinitely - the scheduler stays alive but does nothing
            std::future::pending::<()>().await;
            return;
        }

        // Spawn a task for each scheduled job
        for scheduled_job in &self.schedule {
            let db = self.db.clone();
            let job = scheduled_job.clone();

            let handle = tokio::spawn(async move {
                run_scheduled_job(job, db).await;
            });

            self.task_handles.push(handle);

            debug!("📅 Spawned scheduler task for '{}'", scheduled_job.name);
        }

        // Wait for all tasks to complete (they run indefinitely)
        for (index, handle) in self.task_handles.iter_mut().enumerate() {
            if let Err(e) = handle.await {
                error!("📅 Scheduler task {} failed: {}", index, e);
            }
        }
    }
}

/// Run a single scheduled job in its own loop
async fn run_scheduled_job(scheduled_job: ScheduledJob, db: DatabaseConnection) {
    debug!("📅 Starting scheduler task for '{}'", scheduled_job.name);

    // Parse the cron expression once
    let schedule = parse_cron_schedule(&scheduled_job).expect("Failed to parse cron schedule");

    loop {
        match execute_next_scheduled_run(&scheduled_job, &schedule, &db).await {
            Ok(()) => {
                debug!(
                    "📅 Created scheduled job '{}' for execution",
                    scheduled_job.name
                );
            }
            Err(e) => {
                error!(
                    "❌ Failed to create scheduled job '{}': {}",
                    scheduled_job.name, e
                );
            }
        }
    }
}

/// Parse cron schedule for a job
fn parse_cron_schedule(scheduled_job: &ScheduledJob) -> Result<cron::Schedule, ()> {
    match cron::Schedule::from_str(&scheduled_job.cron_expression) {
        Ok(schedule) => Ok(schedule),
        Err(e) => {
            error!(
                "❌ Invalid cron expression for job '{}': {}",
                scheduled_job.name, e
            );
            Err(())
        }
    }
}

/// Execute the next scheduled run for a job
async fn execute_next_scheduled_run(
    scheduled_job: &ScheduledJob,
    schedule: &cron::Schedule,
    db: &DatabaseConnection,
) -> Result<(), Box<dyn Error>> {
    let now = chrono::Utc::now();

    // Get the next execution time
    let Some(next_execution) = schedule.upcoming(chrono::Utc).take(1).next() else {
        error!(
            "❌ Could not determine next execution time for job '{}'",
            scheduled_job.name
        );
        // Sleep for a minute and try again
        sleep(TokioDuration::from_secs(60)).await;
        return Ok(());
    };

    debug!(
        "🔄 Job '{}' next execution at: {}",
        scheduled_job.name,
        next_execution.format("%Y-%m-%d %H:%M:%S UTC")
    );

    // Sleep until the next execution time
    wait_until_execution_time(next_execution, now).await;

    // Create the job
    create_scheduled_job(scheduled_job, db).await
}

/// Wait until the specified execution time
async fn wait_until_execution_time(
    next_execution: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) {
    let sleep_duration = (next_execution - now).to_std().unwrap_or_default();
    if sleep_duration > Duration::ZERO {
        let tokio_instant = Instant::now() + sleep_duration;
        sleep_until(tokio_instant).await;
    }
}

/// Create a job in the database
async fn create_scheduled_job(
    scheduled_job: &ScheduledJob,
    db: &DatabaseConnection,
) -> Result<(), Box<dyn Error>> {
    let now = chrono::Utc::now().naive_utc();

    let new_job = job::ActiveModel {
        r#type: Set(scheduled_job.job_name.to_string()),
        arguments: Set(scheduled_job.arguments.clone()),
        status: Set(JobStatus::Pending),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    new_job.insert(db).await?;
    Ok(())
}
