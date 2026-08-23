---
title: Deploying monitoring
description: Shipping the collector, console and Prometheus as their own release, in their own cluster.
---

The monitoring stack is a **separate deployment**: its own Helm chart, its own
release, its own Kubernetes context. That separation is the entire point — a
monitoring stack that shares a failure domain with the application goes down
with the outage it exists to report.

## The two commands

```sh
erno deploy init --target monitoring      # generate Dockerfiles, chart, workflow
erno deploy install v0.1.0 --target monitoring
```

Without `--target` both commands act on the application, exactly as before.

`deploy init --target monitoring` writes:

| Path | What |
|---|---|
| `monitoring/Dockerfile` | The collector image (`context: ./monitoring`) |
| `monitoring/ui/Dockerfile` | The console image (`context: ./monitoring/ui`) |
| `monitoring/ui/docker/{nginx.conf,entrypoint.sh}` | Console nginx, proxying `/api/` to the collector |
| `monitoring/deploy/chart/**` | `Chart.yaml`, `values.yaml`, `secrets.example.yaml`, `deploy.toml`, templates |
| `.github/workflows/monitoring.yaml` | Its own build and release workflow, tagged `mon-v*` |

Every path is under `monitoring/`, so it can never overwrite the application's
chart. The one exception is the workflow, which is a separate file rather than a
merge into `build.yaml` — the two deployables have independent release cadences,
and publishing the monitoring chart on every application tag would be wrong.

## Release and chart names

| | Application | Monitoring |
|---|---|---|
| Helm release | `{name}` | `{name}-monitoring` |
| Chart ref | `oci://ghcr.io/{repo}/{name}` | `oci://ghcr.io/{repo}/{name}-monitoring` |
| Chart dir | `chart/` | `monitoring/deploy/chart/` |
| Secrets | `chart/secrets.{env}.yaml` | `monitoring/deploy/chart/secrets.{env}.yaml` |
| Context | `chart/deploy.toml` | `monitoring/deploy/chart/deploy.toml` |

## One host, one origin

The console's nginx serves the SPA and proxies `/api/` to the collector, so the
console, the operator API and the ingest endpoints share one hostname. That is
deliberate: `collector_url` in the application's chart, the browser SDK's URL and
the console's own origin become one string, with one certificate and one CORS
origin to keep straight.

Prometheus is never exposed. It is reached in-cluster, or through the console's
`/prometheus/` location, which gates it behind the collector's operator
credentials — Prometheus has no authentication of its own.

## Values that must match across the two charts

This is the thing most likely to be wrong, and it **fails silently**: a
mismatched token means reports are rejected with a 401 and nothing says so.

| Value | Application chart | Monitoring chart |
|---|---|---|
| Trusted ingest token | `api.error_reporting.ingest_token` | `collector.server_token` |
| Collector URL | `api.error_reporting.collector_url` | `ingress.monitoring.host` |
| Scrape token | `api.metrics_auth_token` | `api.metrics_auth_token` |

`erno deploy init --target monitoring` generates the ingest token and writes it
into **both** `secrets.example.yaml` files, filling the application's only if it
is still empty — an in-use token is never overwritten.

The console's per-source "last report received" timestamps are what make a
mismatch visible after the fact. Check them after the first deploy.

## What cannot come from the chart

`config_rs` parses environment variables without a list separator, so any
**list-valued** config key has to live in `monitoring/config/production.toml`,
which ships inside the image:

- `[cors] allowed_origins` — the app and admin origins that post error reports.
  A missing origin here silently kills browser reporting.
- `[metrics] table_counts`
- `[jobs.workers.*] jobs`

Everything else is set by `_helpers.tpl` as `APP__*` environment variables.

## Recording releases

`erno deploy install` posts to the collector after Helm reports success, so the
release timeline reflects what is actually serving traffic. A chart publish is
not a deploy, which is why this does not live in CI.

It needs two things, and skips with a one-line notice without them:

- `monitoring_url` in the application's `chart/deploy.toml` (plaintext — a
  hostname is not a secret, and the SOPS-encrypted secrets file is unreadable
  from the application deploy path)
- `ERNO_INGEST_TOKEN` in the environment

A failed webhook is a warning, never a failed deploy.

## SOPS

The age key is per **repository**, not per target: one `SOPS_AGE_KEY` secret
decrypts both charts, and each chart directory gets its own `.sops.yaml`
pointing at the same recipient. Re-running `deploy init` reuses the existing
key rather than generating a new one — a fresh key would make every existing
`secrets.<env>.yaml` in the repo undecryptable.

## Graceful shutdown

Both the API and the collector handle `SIGTERM`: they stop accepting
connections, finish in-flight requests, stop claiming new jobs, let running jobs
complete, and flush buffered error reports. The charts set
`terminationGracePeriodSeconds: 30` and a five-second `preStop` sleep — without
that pause the ingress can still route to a process that has stopped accepting,
and graceful shutdown would *increase* 502s rather than removing them.
