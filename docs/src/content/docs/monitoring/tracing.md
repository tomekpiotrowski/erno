---
title: Tracing
description: Sampled request trees in Tempo, queried from the monitoring console.
sidebar:
  order: 8
---

> **Source**: `api/src/tracing_otel.rs`, `cli/src/deploy/render.rs`

Prometheus answers *is this route slow?* Tempo answers *why was this request
slow?* They are not two copies of the same store. Putting SQL text or a user
id on a Prometheus label is how a metrics store dies; a sampled span tree
is the right shape for that data.

Grafana is not in the stack. Tempo is a dumb store, the same way Prometheus
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
`erno deploy`. Unreachable Tempo must not take the API down — the batch
exporter drops.

## Topology

**Development.** `erno dev` starts Tempo next to Prometheus (`127.0.0.1:3200`
query, `127.0.0.1:4318` OTLP/HTTP). Skip with `--no-tempo`.

**Production.** Tempo lives in the monitoring release. Applications **push**
OTLP to `https://<monitoring_host>/otlp/v1/traces` with the trusted server
ingest token. The console queries through `/tempo/`, gated by operator Basic,
the same way `/prometheus/` works. Tempo itself has no authentication.

The public browser token is rejected on this path. Traces are server-side.

## The console

The Performance page keeps every PromQL block and adds a Slow traces table
(`duration > 500ms` in the selected window). Clicking a row opens an indented
span tree. From there, Logs for this trace queries Loki by `trace_id`.
