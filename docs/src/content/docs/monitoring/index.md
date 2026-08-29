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
already true before any of this existed — `erno deploy` put Prometheus inside
the application's own chart.

## What it does today

| Service | Status |
|---|---|
| [Error reporting](/monitoring/error-reporting/) | Ingest, grouping, triage, new-issue email |
| [Releases](/monitoring/releases/) | Deploy tracking, and what each deploy introduced |
| [Subsystem health](/monitoring/subsystem-health/) | Job queue, sync and heartbeat liveness per instance |
| [Uptime checks](/monitoring/uptime/) | Synthetic probes with flap damping |
| [Alerts](/monitoring/alerts/) | Rule engine over errors, uptime and health |
| [Status page](/monitoring/status-page/) | Published document plus a standalone page |
| [Metrics](/monitoring/metrics/) | Prometheus, plus Erno subsystem timings |
| [Tracing](/monitoring/tracing/) | Sampled request trees in Tempo |
| [Logs](/monitoring/logs/) | Loki. Grep, not issue grouping |
| APM | Prometheus aggregates + Tempo traces + Loki logs in this console. RUM is future work |

Prometheus, Tempo and Loki run **here** rather than in the application's
chart, so they do not share a failure domain with what they observe. Grafana
is not in the stack.

## Architecture

One Rust binary, which is an ordinary Erno application. It calls
`boot::<MonitorMigrator, MonitorConfig>(...)` and mounts the collector half of
`erno::error_reporting`; everything underneath — config loading, migrations,
the job queue, the mailer, metrics, health checks, operator Basic auth — comes
from the library. That is what makes running monitoring as a separate service
cheap rather than a second framework to maintain.

The library is split along the deployment seam:

| Half | Contents | Mounted by |
|---|---|---|
| `erno-monitoring` | Ingest, grouping, storage, operator queries | The monitoring binary only |
| `error_reporting::reporter` | The handle applications hold, the HTTP sender, the capture hooks | Every Erno application |

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
| Monitoring collector | 3001 |
| Monitoring console | 4400 |
| Prometheus | 9090 |
| Loki | 3100 |
| Loki (gRPC) | 9096 |
| Tempo (query) | 3200 |
| Tempo (OTLP/HTTP) | 4318 |
| Tempo (gRPC) | 9095 |

The console has a page per service: Issues, Releases, System, Uptime,
Performance, Logs, Statistics, Alerts and Status page.

Running monitoring locally is optional. When it is not up, applications buffer
a bounded number of reports and then drop them; nothing blocks and nothing
fails.

## Operator access

The monitoring console uses HTTP Basic auth against `[admin]` in the monitoring
deployment's own config — deliberately independent of the application's auth
service, which may be exactly what is broken when an operator needs this screen.
