---
title: Metrics
description: Pushed over OTLP into erno-monitoring, and what Erno instruments.
sidebar:
  order: 7
---

> **Source**: `api/src/metrics/`, `api/src/metrics/otlp_push.rs`

## Pushed, not scraped

Metrics ride the same path as every other signal: the application **pushes**
OTLP to erno-monitoring on an interval, authenticated by its ingest token, and
the rows land in that application's store. Nothing reaches into the
application's network, there is no scrape discovery to configure or to fail
silently, and a project starts reporting metrics the moment its token works —
the same moment its errors, traces and logs do.

The pusher lives beside the Prometheus recorder the `metrics` crate macros
already feed: on each interval it renders the recorder's state, computes
**deltas** against the previous snapshot, and POSTs an OTLP batch to
`{collector}/api/otlp/v1/metrics`. Delta temporality is a contract, not a
preference — the collector's rollups fold delta counters and histograms with
plain sums, and cumulative histograms are refused visibly. A process restart
is a non-event: the first snapshot after it simply becomes the new baseline.

```toml
[tracing.otel]
endpoint = "http://localhost:3001/api/otlp"  # the collector; one base for all signals
token = "<project server token>"
metrics_interval_seconds = 15                 # 0 disables the pusher
```

`/metrics` still answers locally for a human with `curl`, but nothing scrapes
it, so `[metrics] auth_token` is no longer a de-facto requirement.

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

**Performance** and **Statistics** live in the monitoring console, rendered
through the collector's query facade: the console asks for finished panels and
knows no metric names at all. Project-specific business panels are project
*configuration* (`business_panels` on the project), not console code. The
admin console links across. Per-request diagnosis is
[tracing](/monitoring/tracing/); log search is [logs](/monitoring/logs/).

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
