---
title: Logs
description: Loki as the log store, queried from the monitoring console. Not issue grouping.
sidebar:
  order: 9
---

> **Source**: `api/src/tracing_otel.rs`, `cli/src/deploy/render.rs`

Loki is Prometheus for logs: a few **bounded** stream labels, the line itself
not indexed, query via LogQL over HTTP. Grafana is only a client — the
monitoring console talks to Loki the same way it already talks to Prometheus.

**This is not Issues.** `{level="error"}` is grep. Fingerprinting, first-seen,
"new error type in this release," scrubbing, and the two ingest tokens stay
in the collector. Grafana Faro writing exceptions into Loki as `kind=error`
would be the same mistake.

## Labels vs metadata

Stream labels must stay a bounded set: `service_name`, `deployment.environment`,
severity. `trace_id`, a route, a user id belong in **structured metadata**
(Loki 3) or in the line — never as stream labels, or Loki falls over the same
way Prometheus would.

```
{service_name="erno-api"} | severity_text="ERROR"
{service_name="erno-api"} | trace_id="4bf92f3577b34da6a3ce929d0e0e4736"
```

## Configuration

Log export is independent of stdout. `[tracing] log_level` still controls
what operators see in `erno dev` and `kubectl logs`. `[tracing.otel] log_level`
controls what is pushed to Loki. Empty means log export is off.

```toml
[tracing.otel]
logs_endpoint = "http://127.0.0.1:3100/otlp"  # exporter appends /v1/logs
log_level = "info"                             # warn in production
```

Empty `logs_endpoint` inherits `endpoint`. Development ships `info`;
production injects `warn` so routine request logs stay out of Loki.

There is no Promtail and no DaemonSet in the application cluster. The
process pushes OTLP, the same way it pushes traces.

## Topology

**Development.** `erno dev` starts Grafana Loki on `127.0.0.1:3100`. Skip with
`--no-loki`. The binary on `PATH` must be Grafana Loki (`loki, version …`).
Debian/Ubuntu's `loki` package is a different program (MCMC linkage analysis)
and will be rejected.

**Production.** Loki lives in the monitoring release with `auth_enabled` — its
name for tenancy, not for authentication — so each project's logs sit under its
own slug as the `X-Scope-OrgID` tenant, set the same way
[traces](/monitoring/tracing/#one-tempo-many-applications) are. Existing
single-tenant chunks are not readable afterwards; delete the volume rather than
migrating it. Applications push to
`https://<monitoring_host>/otlp/v1/logs` with the trusted server ingest
token. The console queries through `/loki/`, gated by operator Basic.
`auth_enabled` is false — single tenant, the app being watched. Do not turn
on multi-tenancy / `X-Scope-OrgID`.

## The console

The Logs page builds LogQL from service, level and a text filter, with a raw
LogQL field as an escape hatch. A trace detail page and an Issue with
`context.trace_id` both link here.
