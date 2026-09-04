---
title: Monitoring
description: A first-party monitoring deployment — errors, metrics, traces and logs, on infrastructure separate from the app.
sidebar:
  order: 0
---

> **Source**: `monitoring/`, `api/src/error_reporting/`

Erno ships a **separate monitoring deployment**: its own binary, its own
database, its own operator console, its own Kubernetes release, running on
infrastructure separate from the application it watches.

The separation is the point. Monitoring that lives inside the deployment it
monitors goes down with it, which is precisely when you need it. That was
already true before any of this existed, when the telemetry stores lived
inside the application's own chart.

## What it does today

| Service | Status |
|---|---|
| [Error reporting](/monitoring/error-reporting/) | Ingest, grouping, triage, new-issue email |
| [Releases](/monitoring/releases/) | Deploy tracking, and what each deploy introduced |
| [Subsystem health](/monitoring/subsystem-health/) | Job queue, sync and heartbeat liveness per instance |
| [Uptime checks](/monitoring/uptime/) | Synthetic probes with flap damping |
| [Alerts](/monitoring/alerts/) | Rule engine over errors, uptime, health and metrics |
| [Status page](/monitoring/status-page/) | Published document plus a standalone page |
| [Metrics](/monitoring/metrics/) | Pushed over OTLP, Erno subsystem timings included |
| [Tracing](/monitoring/tracing/) | Sampled request trees, with a server-side N+1 insight |
| [Logs](/monitoring/logs/) | Grep, not issue grouping |
| APM | Metric aggregates + traces + logs in this console. RUM is future work |

Every signal arrives over **one protocol** — OTLP, authenticated by the
project's ingest token — and lands in **one store** erno-monitoring owns,
running in its release rather than in the application's chart so it does not
share a failure domain with what it observes. Postgres holds the control
plane (projects, issues, rules, incidents); the store holds the telemetry
(spans, logs, metric points, error events, uptime results), each row stamped
with its own per-project retention. Neither Grafana nor a per-signal store
zoo is in the stack.

## Architecture

One Rust binary, which is an ordinary Erno application. It calls
`boot::<MonitorMigrator, MonitorConfig>(...)`, receives every signal on
`/api/otlp/v1/{traces,logs,metrics}`, and serves the console through a typed
query facade — the browser never speaks a query language. Everything
underneath — config loading, migrations, the job queue, the mailer, metrics,
health checks, operator Basic auth — comes from the library. That is what
makes running monitoring as a separate service cheap rather than a second
framework to maintain.

The library is split along the deployment seam:

| Half | Contents | Mounted by |
|---|---|---|
| `erno-monitoring` | OTLP ingest, grouping, storage, the query facade | The monitoring binary only |
| `error_reporting::reporter` | The handle applications hold, the OTLP sender, the capture hooks | Every Erno application |

Collector migrations are deliberately **absent** from `erno_migrations()`. They
belong to the monitoring database; adding them to the framework list would give
every application deployment two large tables it never writes to.

## Ports in development

An application's `erno dev` starts the first four. The rest are the collector's,
started by `erno dev` in the erno-monitoring repository where they are declared
as its components:

| Process | Port |
|---|---|
| Application API | 3000 |
| Application SPA | 4200 |
| Admin console | 4300 |
| Monitoring API (incl. OTLP ingest) | 3001 |
| Monitoring console | 4400 |

The console has a page per service: Issues, Releases, System, Uptime,
Performance, Logs, Statistics, Alerts and Status page.

Running monitoring locally is optional. When it is not up, applications buffer
a bounded number of reports and then drop them; nothing blocks and nothing
fails.

## Operator access

The monitoring console uses HTTP Basic auth against `[admin]` in the monitoring
deployment's own config — deliberately independent of the application's auth
service, which may be exactly what is broken when an operator needs this screen.
