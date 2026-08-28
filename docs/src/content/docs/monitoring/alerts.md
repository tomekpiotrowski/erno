---
title: Alerts
description: A rule engine over errors, uptime and application health — built rather than bundled.
sidebar:
  order: 5
---

> **Source**: `monitoring/src/collector/alerting/`

## Why this is not Alertmanager

Alertmanager only sees Prometheus metrics, so it structurally cannot express
*"a new error type appeared"* or *"this issue is spiking"* — which are the
alerts most worth having here. Bundling it would mean running two alerting
systems, with two config languages and two notification setups, and still
writing the error half by hand.

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
| `promql` | Any instant PromQL query | The first sample the query returns |

## The PromQL source

`promql` makes everything Prometheus scrapes alertable without teaching the
collector each individual signal:

```json
{
  "name": "5xx rate",
  "source": "promql",
  "selector": "sum(rate(http_requests_total{status=~\"5..\"}[5m]))",
  "comparator": "gt",
  "threshold": 0.05
}
```

It reads `[collector.prometheus] url`, which the chart sets to the in-cluster
Service. Empty means the source is unavailable.

Two behaviours worth knowing:

- **An empty result is not zero.** A query returning no samples reads as "no
  value", not as `0` — otherwise a `less than` rule would fire every time a
  series went quiet. `NaN`, `+Inf` and `-Inf` are rejected the same way, since
  `NaN` compares false against every threshold and would make a rule
  undecidable.
- **A Prometheus outage un-fires every PromQL rule.** A failed query reads as
  not breaching, consistent with an unrecognised source. That is the honest
  trade — the alternative is firing every PromQL rule whenever Prometheus
  restarts — but it is a real blind spot. The collector increments
  `erno_alert_source_unavailable_total{source="promql"}` when it happens, and
  Prometheus scrapes the collector, so one rule closes the gap:

  ```
  increase(erno_alert_source_unavailable_total[10m]) > 0
  ```

- **A PromQL selector must name its own project.** Prometheus holds every
  project's metrics, so an unscoped query counts the whole organisation and
  fires one application's alert on another's traffic. The selector has to
  contain the literal matcher `erno_project="<slug>"`; the console's rule
  editor inserts it. A selector without it reads as not breaching and
  increments `erno_alert_source_unavailable_total{source="promql_unscoped"}`,
  so the same catch-all rule above surfaces it.

  The check is a substring test, deliberately. Injecting a matcher into
  arbitrary PromQL — `rate(...)`, `or`, `ignoring(...)` — needs a query parser,
  and a half-right rewrite of someone's alerting expression is worse than
  asking them to be explicit.

  ```
  rate(http_requests_total{erno_project="teryon"}[5m]) > 10
  ```

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
