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
