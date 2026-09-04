---
title: Tracing
description: Sampled request trees in the collector's store, queried from the monitoring console.
sidebar:
  order: 8
---

> **Source**: `api/src/tracing_otel.rs`, `cli/src/deploy/render.rs`

Metrics answer *is this route slow?* Traces answer *why was this request
slow?* They are not two copies of the same store. Putting SQL text or a user
id on a metric label is how a metrics store dies; a sampled span tree
is the right shape for that data.

Grafana is not in the stack. The store stays dumb, the same way the metrics side
is. The operator UI is the monitoring console.

## What is a span

An HTTP request opens a server span named after the **matched route**
(`GET /widgets/{id}`), not the raw URI. `OperationTimer` sites — sync,
storage, email — become children. Jobs are their own traces. sqlx query
logs attach as events on the current span, with elapsed time, without
flooding stdout.

W3C `traceparent` is extracted on the way in and `traceresponse` injected
on the way out. A captured error copies the active `trace_id` onto
`context.trace_id`, so Issues can open the waterfall.

## Configuration

Empty `endpoint` means export is off. A fresh app never tries to push traces.

```toml
[tracing.otel]
endpoint = "http://127.0.0.1:4318"   # exporter appends /v1/traces
token = ""                            # Bearer; production uses the server ingest token
sample_ratio = 1.0                    # 0.1 in production
service_name = "erno"
```

Sampling is parent-based: a sampled parent stays sampled. `1.0` keeps
everything (development); `0.1` is the production default injected by
`erno deploy`. An unreachable collector must not take the API down — the batch
exporter drops.

## Topology

**Development.** The store is declared in erno-monitoring's `erno.toml`, so
`cd erno-monitoring && erno dev` starts it alongside the monitoring API and
console. A product application's `erno dev` starts nothing of the sort and
exports nothing: `[tracing.otel]` is empty in a generated `development.toml`.
Set `endpoint` there to send an app's dev traces to a monitoring API you are
running.

Spans land in erno-monitoring's store. The console renders the waterfall from
the monitoring API's `/traces/{id}` endpoint, which also computes the N+1
insight server-side — eight or more similar statements in one trace, with
literals normalized away, named in the response.

Applications **push** OTLP to `https://<monitoring_host>/api/otlp/v1/traces`
with the trusted server ingest token — straight to the collector, which
authenticates the bearer itself. There is no `auth_request` in the ingest
path, no tenancy header, and no store with an unauthenticated API behind a
proxy: the token is the only tenant statement, and the public browser token
is rejected on this path. Traces are server-side.
