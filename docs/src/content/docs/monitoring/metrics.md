---
title: Metrics
description: Prometheus, moved into the monitoring deployment, and what Erno instruments.
sidebar:
  order: 7
---

> **Source**: `api/src/metrics/`, `cli/src/deploy/render.rs`

## Prometheus moved

It used to run inside the application's own deploy, which meant the thing
doing the observing shared a failure domain with the thing being observed. It
now belongs to the monitoring deployment.

Two consequences:

- **Scraping crosses the network.** `/metrics` is no longer reached in-cluster,
  so `[metrics] auth_token` is required in practice — an internet-reachable
  metrics endpoint should not be open.
- **The collector is scraped too.** An operator needs to know when the thing
  doing the watching is itself struggling.

```toml
# monitoring/deploy/config.toml
[production.scrape]
target = "api.example.com:443"
scheme = "https"
```

`api.metrics_auth_token` in `monitoring/deploy/secrets.<env>.yaml` must equal
the application's `[metrics] auth_token`.

In development `erno dev` writes a config that scrapes the application, and the
collector as well when the project has a `monitoring/` directory. A project
without one does not get a target pointed at a port nothing is listening on —
a permanently red scrape target teaches people to ignore the scrape health page.

## What is instrumented

### HTTP

| Metric | Type | Labels |
|---|---|---|
| `http_requests_total` | counter | `method`, `path`, `status` |
| `http_request_duration_seconds` | histogram | `method`, `path`, `status` |
| `http_requests_in_flight` | gauge | — |

`path` is the *matched route*, not the raw URI, so `/decks/{id}` stays one
series instead of one per deck.

### Erno subsystems

These are the numbers a generic exporter cannot produce, because they require
knowing what a job, a delta pull or an outbox send actually is.

| Metric | Type | Labels |
|---|---|---|
| `erno_jobs_queue_wait_seconds` | histogram | `job_type` |
| `jobs_execution_duration_seconds` | histogram | `job_type` |
| `jobs_executed_total` | counter | `job_type`, `result` |
| `erno_sync_delta_duration_seconds` | histogram | `entity`, `outcome` |
| `erno_sync_delta_rows` | histogram | `entity` |
| `erno_storage_upload_duration_seconds` | histogram | `backend`, `outcome` |
| `erno_storage_download_duration_seconds` | histogram | `backend`, `outcome` |
| `erno_storage_uploaded_bytes_total` | counter | `backend` |
| `erno_email_send_duration_seconds` | histogram | `template`, `outcome` |
| `erno_email_send_total` | counter | `template`, `outcome` |

Two of these are worth explaining.

**Queue wait**, not queue depth, is what predicts user-visible lateness. A
thousand jobs enqueued a second ago is a healthy burst; one job waiting ten
minutes is not. It is measured from when a job *became runnable* rather than
when it was created, so a job deliberately scheduled for later — or backing off
after a failure — is not counted as late until its time arrives.

**Delta rows** is the signal that a client is about to have a bad time. A pull
returning tens of thousands of rows means someone has been offline a long while,
or a backfill has rewritten everything.

### Gauges

`db_stats_task` publishes pool, table-count, job-queue and user gauges on an
interval, and `erno_jobs_*` / `erno_sync_*` liveness gauges come from the same
readings the [System page](/monitoring/subsystem-health/) shows.

## Label cardinality

Every label above is a bounded set: a job type, an entity name, a storage
backend, an email template. None is a user id, a file name or a URL. That is a
deliberate constraint rather than an accident — unbounded labels are the usual
way a metrics store falls over, and the `OperationTimer` helper in
`api/src/metrics/timing.rs` exists partly to keep the shape consistent and the
dimension obvious at each call site.

## Dashboards

**Performance** and **Statistics** moved from the admin console to the
monitoring console, since that is where Prometheus now lives. The admin console
links across to them.

The admin **Database** page still reads table-count gauges and stays where it
is: it is about the application's own data rather than about its performance.

## Adding your own

The `metrics` crate is re-exported, so application code can record its own:

```rust
metrics::counter!("myapp_widgets_created_total", "kind" => kind).increment(1);
```

For anything timed, prefer the shared helper so the metric shape matches the
rest:

```rust
let timer = erno::metrics::OperationTimer::start(
    "myapp_import_duration_seconds",
    "myapp_import_total",
    "format",
    format_name,
);
let result = do_the_import().await;
timer.finish(&result);
```

It is deliberately not `Drop`-based: an operation cancelled mid-flight is not
the same as one that succeeded or failed, and recording it as either would be
worse than not recording it.
