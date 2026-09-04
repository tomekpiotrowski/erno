---
title: Alerts
description: A rule engine over errors, uptime, health and metrics — built rather than bundled.
sidebar:
  order: 5
---

> **Source**: `erno-monitoring: api/src/collector/alerting/`

## Why this is not Alertmanager

Alertmanager only sees Prometheus metrics, so it structurally cannot express
*"a new error type appeared"* or *"this issue is spiking"* — which are the
alerts most worth having here. Bundling it would mean running two alerting
systems, with two config languages and two notification setups, and still
writing the error half by hand — and with the telemetry store living inside
erno-monitoring, there is no Prometheus for Alertmanager to sit behind
anyway.

Erno already has the parts: cron-scheduled work, advisory-lock singletons, and
a mailer.

## Rules

A rule reads a number from a source, compares it with a threshold, and holds
the result for a while before believing it.

| Source | Selector | The number |
|---|---|---|
| `errors` | `new_issues` | Error types first seen inside the window |
| `errors` | `all`, `api`, `app`, `admin` | Error events in the window |
| `uptime` | `all`, or a check name | Checks currently down |
| `subsystem` | `down` (default), `degraded` | Application instances not healthy |
| `metric` | `{ "metric", "agg"?, "where"? }` as JSON | An aggregate over the window |

## The metric source

`metric` makes everything an application pushes alertable without teaching the
collector each individual signal. The selector is a typed description, not a
query language:

```json
{
  "name": "5xx rate",
  "source": "metric",
  "selector": "{ \"metric\": \"http_requests_total\", \"agg\": \"sum\", \"where\": { \"status\": \"500\" } }",
  "comparator": "gt",
  "threshold": 100,
  "window_seconds": 300
}
```

Aggregates: `sum`, `avg`, `max`, `min`, `last` (default), `p50`, `p95`, `p99`
— the quantiles read the histogram rollup. Selectors are validated on create,
so a rule that would never evaluate is refused at the door rather than
discovered silent.

Three behaviours worth knowing:

- **Scoping is structural.** The rule row carries its project id and every
  query binds it. There is no matcher to remember and no query language for
  an unscoped selector to hide in — which is the whole advance over the
  PromQL source this replaced.
- **An empty result is not zero.** A metric with no samples in the window
  reads as "no value", not as `0` — otherwise a `less than` rule would fire
  every time a series went quiet.
- **A store outage reads as not breaching**, consistent with an unrecognised
  source, and counts `erno_alert_source_unavailable_total{source="metric"}`
  — itself a pushed metric, so one catch-all rule over it closes the gap.

```sh
curl -X POST https://monitoring.example.com/api/collector/projects/teryon/alerts \
  -u operator:secret \
  -H 'content-type: application/json' \
  -d '{"name":"New error types","source":"errors","selector":"new_issues",
       "comparator":"gt","threshold":0,"window_seconds":3600,
       "for_seconds":300,"notify_email":"ops@example.com"}'
```

An **unrecognised source reads as zero**, so a typo in a rule cannot page
anyone.

## The three things that stop it becoming noise

Alert fatigue is how monitoring platforms actually die, so each of these is
deliberate:

**`for_seconds`** — a breach must persist before it is believed. A rule moves
`ok → pending → firing`, and a breach that clears while pending never notifies
at all. Nobody was told it started, so nobody is told it stopped.

**`repeat_seconds`** — a rule that stays firing re-notifies on a schedule
(default 4h) rather than on every evaluation. Zero means notify once.

**Silences** — suppress notifications for a while without suppressing
evaluation. The console keeps showing the truth, which is what an operator
needs while working on the problem. A silenced recovery is not announced
either, so silencing does not produce a stray "resolved" message.

Recovery **is** announced for anything that was announced, so nobody chases a
problem that has already gone.

## Delivery

Email through the configured mailer, and/or a webhook receiving:

```json
{ "status": "firing", "rule": "New error types", "severity": "warning",
  "source": "errors", "selector": "new_issues", "threshold": 0,
  "description": "3 new error type(s) in the last 1h" }
```

A rule without `notify_email` falls back to `[collector.alerts] recipient`.

Notification failures are counted and dropped rather than retried — the rule is
evaluated again shortly, and a stale duplicate is worse than a gap.

## One evaluator per deployment

The loop runs under an advisory lock. Replicas each evaluating and each
notifying would multiply every alert by the replica count, which is the fastest
possible route to people muting the whole system.

## New-issue email

The first sighting of an error fingerprint is mailed separately by the ingest
writer, throttled by `[collector.alerts]` — see
[Error reporting](/monitoring/error-reporting/#alerting). It predates this
engine and stays separate because it has information the engine does not: which
specific issue is new.
