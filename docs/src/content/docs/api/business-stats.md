---
title: Business Stats
description: Business metrics as Prometheus gauges and counters
sidebar:
  order: 16
---

> **Source**: `api/src/metrics/db_stats.rs`, `api/src/admin_events.rs`

Business trends are Prometheus time series, scraped from `/metrics`. There is no `stat_snapshot` table and no daily snapshot job.

## Gauges (every ~30s)

Updated by `db_stats_task` from cheap queries on `users` / subscription tables, and from `pg_stat_user_tables.n_live_tup` for table sizes (never `COUNT(*)` on unbounded tables).

| Metric | Meaning |
|--------|---------|
| `erno_users_total` | User count |
| `erno_users_email_verified` | Verified emails |
| `erno_users_paid` / `_trial` / `_gift` / `_none` | Mix |
| `erno_users_paid{plan}` | Paid by plan |
| `erno_users_active_1d` / `_7d` / `_30d` | `last_active_at` windows |
| `erno_users_past_due` / `_canceled` / `_cancel_at_period_end` | Stripe signals |
| `erno_storage_bytes` / `erno_files_total` | Storage |
| `db_table_count{table}` | Approximate row counts |

## Counters (on write)

| Metric | When |
|--------|------|
| `erno_users_registered_total` | New account |
| `erno_users_verified_total` | Email verified / admin activate |
| `erno_users_deleted_total` | Account purge |
| `erno_subscriptions_activated_total` | Stripe checkout or trial |
| `erno_subscriptions_canceled_total` | Stripe deleted |
| `erno_subscriptions_gifted_total` | Admin gift |

The same moments write an `admin_event` row (operator feed). Deleted users keep that event.

Apps add their own counters (Cubeast: `cubeast_solves_created_total`, `cubeast_sessions_created_total`) and optional collectors via `BootConfig::with_metrics_collector` — never a large `COUNT(*)`.

The admin Business page charts these via PromQL (`increase(...[24h])` for rates).
