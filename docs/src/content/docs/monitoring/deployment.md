---
title: Deploying monitoring
description: Shipping the collector, console and Prometheus as their own release, in their own cluster.
---

The monitoring stack is a **separate deployment**: its own `erno deploy`
target, its own release, its own Kubernetes context. That separation is the
entire point — a monitoring stack that shares a failure domain with the
application goes down with the outage it exists to report.

## The two commands

```sh
erno deploy init --target monitoring      # generate Dockerfiles, config, workflow
erno deploy setup --target monitoring     # cert-manager + ingress-nginx on that cluster
erno deploy install v0.1.0 --target monitoring
```

Without `--target` both commands act on the application, exactly as before.

`deploy init --target monitoring` writes:

| Path | What |
|---|---|
| `monitoring/Dockerfile` | The collector image (`context: ./monitoring`) |
| `erno-monitoring: app/Dockerfile` | The console image (`context: ./app`) |
| `erno-monitoring: app/docker/{nginx.conf,entrypoint.sh}` | Console nginx, proxying `/api/` to the collector |
| `monitoring/deploy/config.toml` | Context, host, scrape target, Prometheus |
| `monitoring/deploy/secrets.example.yaml` | Collector secrets, registry pull creds |
| `.github/workflows/monitoring.yaml` | Its own build and release workflow, tagged `mon-v*` |

Every path is under `monitoring/`, so it can never overwrite the application's
`deploy/`. The one exception is the workflow, which is a separate file rather
than a merge into `build.yaml` — the two deployables have independent release
cadences.

## Release names

| | Application | Monitoring |
|---|---|---|
| Release | `{name}` | `{name}-monitoring` |
| Images | `…/{api,app,www,admin}:<tag>` | `…/monitoring:<tag>`, `…/monitoring-ui:<tag>` |
| Config | `deploy/` | `monitoring/deploy/` |
| Secrets | `deploy/secrets.{env}.yaml` | `monitoring/deploy/secrets.{env}.yaml` |
| Context | `deploy/config.toml` | `monitoring/deploy/config.toml` |

## One host, one origin

The console's nginx serves the SPA and proxies `/api/` to the collector, so the
console, the operator API and the ingest endpoints share one hostname. That is
deliberate: `monitoring_url` in the application's `deploy/config.toml`, the
browser SDK's URL and the console's own origin become one string, with one
certificate and one CORS origin to keep straight.

Prometheus, Tempo and Loki are never exposed. They are reached in-cluster, or
through the console's `/prometheus/`, `/tempo/` and `/loki/` locations, which
gate them behind the collector's operator credentials — those stores have no
authentication of their own.

Applications **push** traces and logs to `/otlp/v1/traces` and `/otlp/v1/logs`
on the same host, authenticated with the trusted **server** ingest token
(`Authorization: Bearer`). The public browser token is rejected. nginx
`auth_request`s `/api/otlp/auth` on the collector, then proxies to Tempo :4318
or Loki :3100. That GET is not rate-limited: every replica shares the console
pod's IP, so a per-IP quota would be a global ingest ceiling.

## Values that must match across the two deployments

This is the thing most likely to be wrong, and it **fails silently**: a
mismatched token means reports are rejected with a 401 and nothing says so.

| Value | Application | Monitoring |
|---|---|---|
| Trusted ingest token | `api.ingest_token` | the project's server token (minted by the collector) |
| Collector URL | `monitoring_url` in `deploy/config.toml` | `hosts.monitoring` |
| Scrape token | `api.metrics_auth_token` | `api.metrics_auth_token` |
| OTLP token | `api.ingest_token` (same as errors) | the same project server token |

`erno deploy init --target monitoring` generates the ingest token and writes it
into **both** `secrets.example.yaml` files, filling the application's only if it
is still empty — an in-use token is never overwritten.

The console's per-source "last report received" timestamps are what make a
mismatch visible after the fact. Check them after the first deploy.

## What cannot come from the deploy config

`config_rs` parses environment variables without a list separator, so any
**list-valued** config key has to live in `monitoring/config/production.toml`,
which ships inside the image:

- `[cors] allowed_origins` — the app and admin origins that post error reports.
  A missing origin here silently kills browser reporting.
- `[metrics] table_counts`
- `[jobs.workers.*] jobs`

Everything else is set as `APP__*` environment variables by the CLI renderer.

## Recording releases

`erno deploy install` posts to the collector after the rollout is ready, so the
release timeline reflects what is actually serving traffic. Publishing images
is not a deploy, which is why this does not live in CI.

It needs two things, and skips with a one-line notice without them:

- `monitoring_url` in the application's `deploy/config.toml` (plaintext — a
  hostname is not a secret)
- `ERNO_INGEST_TOKEN` in the environment

A failed webhook is a warning, never a failed deploy.

## SOPS

The age key is per **repository**, not per target: one `SOPS_AGE_KEY` secret
decrypts both secret files, and each deploy directory gets its own `.sops.yaml`
pointing at the same recipient. Re-running `deploy init` reuses the existing
key rather than generating a new one — a fresh key would make every existing
`secrets.<env>.yaml` in the repo undecryptable.

## Graceful shutdown

Both the API and the collector handle `SIGTERM`: they stop accepting
connections, finish in-flight requests, stop claiming new jobs, let running jobs
complete, and flush buffered error reports. The renderer sets
`terminationGracePeriodSeconds: 30` and a five-second `preStop` sleep — without
that pause the ingress can still route to a process that has stopped accepting,
and graceful shutdown would *increase* 502s rather than removing them.
