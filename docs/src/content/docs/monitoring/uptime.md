---
title: Uptime checks
description: Synthetic probes, with flap damping that keeps them trustworthy.
sidebar:
  order: 4
---

> **Source**: `erno-monitoring: api/src/collector/uptime/`

Without probes, a status page is a manual claim. With them it is an observation.

## Defining a check

```sh
curl -X POST https://monitoring.example.com/api/collector/projects/teryon/uptime \
  -u operator:secret \
  -H 'content-type: application/json' \
  -d '{"name":"API liveness","url":"https://api.example.com/liveness","interval_seconds":60}'
```

| Field | Default | Notes |
|---|---|---|
| `url` | — | Must be `http://` or `https://` |
| `method` | `GET` | |
| `expected_status` | `200` | |
| `timeout_ms` | `10000` | Clamped to 100–60 000 |
| `interval_seconds` | `60` | **Floored at 10**, so a typo cannot hammer a target |
| `assert_body_contains` | — | Catches a server returning 200 with an error page |
| `failure_threshold` | `2` | Consecutive failures before the check is called down |

Redirects are **not** followed. A check that passed by landing somewhere else
entirely is not verifying what the operator asked about.

## Flap damping

Going **down** requires `failure_threshold` consecutive failures. Coming **up**
requires a single success.

The asymmetry is deliberate. One dropped packet must not wake anyone, so
failure needs corroboration — but a service that is answering is answering, and
there is no value in making people wait to hear it.

Only a *transition* is logged or alerted on. A check that stays down does not
re-announce itself on every probe.

## Where probes run

From the **monitoring deployment**. That verifies the application from outside
the application, but not from outside the monitoring provider: a network fault
on the monitoring side reads as an application outage.

This is a real limitation, stated rather than hidden. Probing from more than one
place would fix it and is not implemented.

## Retention

Raw results are kept 7 days; the ratios and percentiles the console shows are
computed from them, and the status page keeps 90 days of per-day rollups. Probe
results are high volume and only interesting while recent.

A check that has never run reports `null` uptime rather than zero — unmeasured
is not the same as broken.
