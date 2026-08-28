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
query, `127.0.0.1:4318` OTLP/HTTP) and sets `APP_TRACING__OTEL__ENDPOINT` on
the API (and the collector) so traces flow even when `[tracing.otel]` is
empty in `development.toml`. Skip with `--no-tempo`. The config it writes is
Tempo 3.0 (`live_store` / `backend_scheduler`); a 2.x binary that still
expects `ingester` / `compactor` will not start.

**Production.** Tempo 3.0 lives in the monitoring release (`grafana/tempo:3.0.3`,
`live_store` / `backend_scheduler`, same shape as `erno dev`). Applications
**push** OTLP to `https://<monitoring_host>/otlp/v1/traces` with the trusted
server ingest token. The console queries through `/tempo/`, gated by operator
Basic, the same way `/prometheus/` works. Tempo itself has no authentication.
A 2.x volume is not readable after this upgrade.

The public browser token is rejected on this path. Traces are server-side.

## One Tempo, many applications

Tempo runs with `multitenancy_enabled`, and each project's traces live under its
slug as the `X-Scope-OrgID` tenant. nginx cannot stamp a project onto an OTLP
protobuf body, but the tenant is a header, so the collector resolves it during
the `auth_request` it already performs and nginx copies it onto the push:

```
POST /otlp/v1/traces          Authorization: Bearer erns_…
  → GET /api/otlp/auth        200, X-Scope-OrgID: teryon
  → POST tempo:4318/v1/traces X-Scope-OrgID: teryon   (Authorization stripped)
```

The tenant comes from the token lookup and **never from the caller** — nginx
overwrites whatever arrived. A stolen server token can write traces as its own
project and no other.

The collector's own traces do not pass through nginx: it pushes in-cluster,
straight to Tempo. The chart sets `APP__TRACING__OTEL__TENANT=monitoring` for
it, which is the project its boot seed creates. An application must leave
`[tracing.otel] tenant` empty — setting it would be naming a tenant for itself,
which is the thing the header is there to prevent.

:::caution[Turning this on wipes existing traces]
Blocks written before multi-tenancy have no tenant and are not readable after
it. Pre-1.0, delete the Tempo and Loki PVCs on the deploy that enables it
rather than migrating them.
:::

## The console

The Performance page keeps every PromQL block and adds a Slow traces table
(`duration > 500ms` in the selected window). Clicking a row opens an indented
span tree. sqlx query events on a span are listed, and eight or more similar
statements become an N+1 callout. From there, Logs for this trace queries Loki
by `trace_id`.

`erno dev`'s TTY dashboard is a second client of the same Tempo instance
(`http://127.0.0.1:3200`). It does not add a `/dev/traces` store.
