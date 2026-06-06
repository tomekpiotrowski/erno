---
title: Jobs
description: Background job scheduling with cron expressions and advisory locks
sidebar:
  order: 5
---

> **Source**: `api/src/jobs/`, `api/src/job_queue/`

Erno provides a background job system built on PostgreSQL. Jobs are persisted in the database and executed by worker goroutines. Advisory locks ensure only one worker runs a given job at a time across multiple app instances.

## Defining a job

Implement the `Job` trait:

```rust
use erno::jobs::{Job, JobError};
use erno::app::App;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct SendEmailArguments {
    pub user_id: Uuid,
}

pub struct SendWelcomeEmailJob;

impl Job for SendWelcomeEmailJob {
    type Arguments = SendEmailArguments;

    fn name() -> &'static str {
        "send_welcome_email"
    }

    async fn execute(app: &App, args: Self::Arguments) -> Result<(), JobError> {
        // Send email using app.mailer
        Ok(())
    }
}
```

### Error handling

Return `JobError::FailPermanently` for non-retryable failures (bad data, invalid state). Return `JobError::TryAgainLater` to signal that the job should be retried later.

## Registering jobs

```rust
fn job_registry() -> JobRegistry {
    let mut registry = JobRegistry::new();
    registry.register_job::<SendWelcomeEmailJob>();
    registry
}
```

## Enqueuing jobs

```rust
use erno::job_queue;

// Inside a handler or another job
job_queue::enqueue(
    &app.db,
    SendWelcomeEmailJob::name(),
    &SendEmailArguments { user_id: user.id },
).await?;
```

## Scheduling jobs (cron)

Use `ScheduledJob` to define cron-driven jobs. The cron expression is in 6-field format (seconds included):

```rust
use erno::jobs::scheduled_job::ScheduledJob;

fn job_schedule() -> Vec<ScheduledJob> {
    vec![
        ScheduledJob {
            name: "cleanup".to_string(),
            job_name: CleanupJob::name(),
            arguments: serde_json::Value::Null,
            cron_expression: "0 0 * * * *".to_string(), // every hour
        },
    ]
}
```

Scheduled jobs are enqueued by the scheduler process that runs alongside the HTTP server.

## Advisory locks

Before executing a job, Erno acquires a PostgreSQL advisory lock keyed on the job type. This prevents duplicate execution when multiple app instances are running. The lock is released automatically when the job completes or fails.

## Retries and failure handling

When a job returns `JobError::TryAgainLater` (or times out), the worker schedules a retry with exponential backoff. `JobError::FailPermanently` skips retries. Once a job exhausts its retries it is marked **failed** (terminal) and the failure hooks below run.

### Retry settings and precedence

Four settings control retry/timeout behaviour: `job_timeout`, `max_retries`, `base_retry_delay_seconds`, and `retry_backoff_multiplier`. Each is resolved with the precedence:

**per-job override → worker-pool override → app-wide `[jobs.defaults]`**

App-wide defaults live in config:

```toml
# App-wide defaults — inherited unless overridden.
[jobs.defaults]
job_timeout = 300
max_retries = 4
base_retry_delay_seconds = 60
retry_backoff_multiplier = 5

# A pool may override any subset; omitted keys inherit [jobs.defaults].
[jobs.workers.default]
jobs = []
count = 2
# max_retries = 8   # e.g. override just this pool
```

The retry delay before attempt *n* is `base_retry_delay_seconds * retry_backoff_multiplier^n`.

### Per-job overrides

Override any setting for a specific job by implementing the optional `Job` methods (each defaults to `None` = inherit):

```rust
impl Job for ChargeCardJob {
    type Arguments = ChargeArgs;
    fn name() -> &'static str { "charge_card" }
    async fn execute(app: &App, args: Self::Arguments) -> Result<(), JobError> { /* ... */ Ok(()) }

    // This job is money-sensitive: retry more, and longer.
    fn max_retries() -> Option<i32> { Some(10) }
    fn base_retry_delay_seconds() -> Option<u64> { Some(30) }
}
```

### Failure hooks

Two hooks fire when a job permanently fails (both run, in addition to the error log + `jobs_executed_total{result="failed"}` metric):

- **Per-job** — override `Job::on_permanent_failure` for job-specific handling (alerting, compensation):

  ```rust
  async fn on_permanent_failure(app: &App, arguments: &serde_json::Value, error: &str) {
      // e.g. notify, write an audit row, enqueue a compensating job
  }
  ```

- **App-wide** — register a `JobFailureHandler` to be notified for *every* job type:

  ```rust
  use erno::jobs::failure_handler::JobFailureHandler;

  struct AlertOnFailure;
  #[async_trait::async_trait]
  impl JobFailureHandler for AlertOnFailure {
      async fn on_permanent_failure(&self, job_type: &str, arguments: &serde_json::Value, error: &str) {
          // send an alert
      }
  }

  // wire it during boot
  BootConfig::new(app_info, app_router, registry, schedule)
      .on_job_failure(std::sync::Arc::new(AlertOnFailure));
  ```
