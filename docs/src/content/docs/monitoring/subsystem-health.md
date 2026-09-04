---
title: Subsystem health
description: Liveness signals that require knowing how Erno works.
sidebar:
  order: 3
---

> **Source**: `api/src/health/`, `erno-monitoring: api/src/collector/health.rs`

Generic monitoring sees HTTP status codes and CPU. These signals require
knowing what Erno is doing: that jobs move through a lifecycle, that a
`running` row older than its timeout means a worker died holding it, that a
growing `sync_push_queue` means clients are about to see stale data.

## How readings travel

Each application instance gathers a reading on an interval and **pushes** it to
the collector. Push rather than scrape, for two reasons: it works when the
application is not reachable from outside, and a heartbeat that *stops* is
itself the clearest signal that something is badly wrong.

The same numbers are also published as metric gauges, so a deployment can
use either or both.

```toml
[error_reporting]
report_health = true
health_interval_seconds = 30
```

Instances are identified by `HOSTNAME`, which is what a container orchestrator
sets. Only the latest reading per instance is kept — this is a liveness view,
not a time series. The metrics store is the right place for history.

## What is judged

| Subsystem | Degraded | Down |
|---|---|---|
| **jobs** | oldest waiting job past `job_age_degraded_seconds` (120s), or `job_failures_degraded` (10) permanent failures in an hour | oldest waiting job past `job_age_down_seconds` (600s), or **any** job claimed by a worker that stopped reporting |
| **sync** | backlog past `sync_depth_degraded` (10 000), or oldest unpushed row past `sync_age_degraded_seconds` (300s) | — |
| **database** | never | never |
| **heartbeat** | reading could not be parsed | no reading for `heartbeat_stale_seconds` (180s) |

Two of these deserve explanation.

**Queue depth alone is never an alert.** A thousand jobs enqueued a second ago
is a healthy burst. Age is what distinguishes a busy queue from a stopped one,
so the thresholds are on age.

**Database saturation is reported but never judged.** Erno holds connections for
the whole process lifetime — the sync listener, the websocket listener, each job
worker — so zero idle connections is the normal steady state, not saturation. A
single reading cannot tell the two apart, and flagging it would leave the
healthy case permanently amber. Sustained saturation is a question for a rule
over time series, not for a point-in-time verdict.

## Stale beats stale-and-confident

A heartbeat that stopped outranks whatever the last reading said. The numbers
describe a moment that has passed, and reporting them as current would be worse
than reporting nothing.

Instances that have not reported for `instance_retention_seconds` (24h) are
forgotten by the retention sweep. Without that, every replica a deployment ever
had accumulates in the console and a rolling deploy quietly doubles the list.
