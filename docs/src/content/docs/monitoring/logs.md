---
title: Logs
description: Log search in the collector's store. Grep, not issue grouping.
sidebar:
  order: 9
---

> **Source**: `api/src/tracing_otel.rs`, `cli/src/deploy/render.rs`

The log store is grep-shaped: a few **bounded** columns, the line itself
not indexed, query via LogQL over HTTP. Grafana is only a client — the
monitoring console asks the collector, which composes the query.

**This is not Issues.** `{level="error"}` is grep. Fingerprinting, first-seen,
"new error type in this release," scrubbing, and the two ingest tokens stay
in the collector. A log line marked `kind=error`
would be the same mistake.

## Labels vs metadata

Stream labels must stay a bounded set: `service_name`, `deployment.environment`,
severity. `trace_id`, a route, a user id belong in **structured metadata**
or in the line — never as high-cardinality indexed columns, or the store
falls over the same way any metrics store would.

```
{service_name="erno-api"} | severity_text="ERROR"
{service_name="erno-api"} | trace_id="4bf92f3577b34da6a3ce929d0e0e4736"
```

## Configuration

Log export is independent of stdout. `[tracing] log_level` still controls
what operators see in `erno dev` and `kubectl logs`. `[tracing.otel] log_level`
controls what is exported. Empty means log export is off.

```toml
[tracing.otel]
endpoint = "http://localhost:3001/api/otlp"  # the collector; appends /v1/logs
token = "<project server token>"
log_level = "info"                           # warn in production
```

Logs ride the same endpoint and token as traces (`logs_endpoint` exists to
split them, and empty inherits `endpoint`). Development ships `info`;
production injects `warn` so routine request logs stay out of the store.

There is no Promtail and no DaemonSet in the application cluster. The
process pushes OTLP, the same way it pushes traces.

## Topology

**Development.** `erno dev` in the erno-monitoring repository starts that
application's store (declared in its `erno.toml`). Log records land there
with a body index, so "Contains" can skip rather than scan. A product
application's `erno dev` starts no store of its own.

**Production.** The same store, in the monitoring release, as extra YAML in
that repository. Each row is stamped with its project's retention at ingest
and expires on its own; there is no retention daemon and no tenancy header —
the ingest token is the only tenant statement, and the console reads through
the monitoring API's authenticated query facade.

## The console

The Logs page builds LogQL from service, level and a text filter, with a raw
LogQL field as an escape hatch. A trace detail page and an Issue with
`context.trace_id` both link here.
