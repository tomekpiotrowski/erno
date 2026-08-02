---
title: Business Stats
description: Daily business-metrics snapshots stored as plain Postgres rows
sidebar:
  order: 16
---

> **Source**: `api/src/business_stats/`

Erno can track how your SaaS app is trending as a *business* — signups, subscriber mix, churn signals, storage, and active users — without a separate time-series database or a tool like Grafana. A daily job computes a snapshot of metrics and writes them as plain rows to a `stat_snapshot` table; the [Admin TUI](/api/console) charts them as terminal sparklines.

## How it works

`BusinessStatsSnapshotJob` runs once a day (recommended: 03:00 UTC, an off-peak hour) and inserts one `stat_snapshot` row per metric, all sharing the same `captured_at` timestamp:

| Column | Notes |
|--------|-------|
| `captured_at` | Shared by every row from one run |
| `metric` | e.g. `total_users`, `paid_active_count` |
| `dimension` | Optional sub-key (e.g. a plan name for per-plan breakdowns); `NULL` for aggregates |
| `value` | Stored as `double precision` so future ratio/percentage metrics fit without a schema change |

This is a narrow (EAV-style) table rather than one wide row per run: adding a new metric later is an `INSERT`, not a migration, and the Admin TUI charts any metric it finds without code changes.

## Metrics computed

| Metric | Dimension | Meaning |
|--------|-----------|---------|
| `total_users` | — | Total user count |
| `new_users_since_last` | — | Users created since the previous run (or the last 24h, on the first run) |
| `email_verified_count` | — | Users with a verified email |
| `paid_active_count` | — and per plan | Users with an active Stripe subscription (from the cached `subscription_type`/`subscription_plan` on `users`) |
| `trial_active_count` | — | Users on an active trial |
| `gift_active_count` | — | Users on an active gift subscription |
| `no_sub_count` | — | Users with no subscription of any kind |
| `past_due_count` | — | Distinct users with a `stripe_subscriptions` row in `past_due` status |
| `canceled_count` | — | Cumulative, all-time count of subscription rows ever marked `canceled` (not a rolling window — see limitations) |
| `cancel_at_period_end_count` | — | Active subscriptions already flagged to cancel at period end — a leading churn indicator |
| `total_storage_bytes` / `total_file_count` | — | Aggregate file storage, deduplicated across multiply-attached files |
| `active_users_1d` / `_7d` / `_30d` | — | Users with `last_active_at` inside the window — see [Activity tracking](/api/authentication#activity-tracking) |

Operational health (job/email failure rates) is intentionally **not** included — that's a different audience and refresh cadence than year-over-year business trends. It can be added later as more `metric` rows in the same table, with no schema change.

## Known limitations

Account deletion in Erno is a **hard delete with no tombstone** — a user who signs up and is deleted between two daily snapshots leaves no trace in absolute counts. This is a deliberate simplicity trade-off: no insert-only event log is kept alongside the snapshots. If that blind spot matters for your app, consider adding your own event log.

`canceled_count` is cumulative (all-time), not a rolling window, since `stripe_subscriptions` rows accumulate historically per user.

## Enabling it

The job is **not** registered automatically (like `ExpireSubscriptionsJob`, it stays fully opt-in so a library upgrade doesn't newly trip the [worker-coverage panic](/api/jobs/#worker-coverage-check) for existing apps). Three steps:

```rust
use erno::business_stats::{business_stats_scheduled_job, BusinessStatsSnapshotJob};

fn job_registry() -> JobRegistry {
    let mut registry = JobRegistry::new();
    registry.register_job::<BusinessStatsSnapshotJob>();
    registry
}

fn job_schedule() -> Vec<ScheduledJob> {
    vec![business_stats_scheduled_job()]
}
```

```toml
# config/*.toml
[jobs.workers.default]
jobs = ["business_stats_snapshot"]
```

Omitting the config step is the easy mistake to make — the app boots but the job never runs, since every registered job type must be claimed by a worker pool.

## Viewing the data

Open the Admin TUI (`cargo run --features admin -- admin`) and press `s` from the Dashboard to see the Stats screen — see [Admin TUI](/api/console#stats).
