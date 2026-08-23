---
title: Status page
description: A public page that keeps telling the truth when the monitoring stack is down.
sidebar:
  order: 6
---

> **Source**: `api/src/error_reporting/collector/status/`, `monitoring/status/`

## The constraint everything follows from

**A status page that goes down during an outage is worse than having none**,
because readers conclude nothing is wrong.

So the collector does not serve the page. It periodically **publishes** a static
JSON document, and the page — a single dependency-free HTML file, hosted
separately — reads only that. The last published document is still there when
the collector is not.

```toml
[collector.status]
enabled = true
name = "Acme status"
# In production this must be somewhere served independently of this deployment
# — object storage behind a CDN — or the page goes down with the collector.
output_path = "status/status.json"
refresh_seconds = 30
```

The document is written to a temporary file and renamed, because a reader that
caught a half-written document would show nonsense at exactly the wrong moment.

For local development the collector also exposes
`GET /api/collector/status.json`, unauthenticated. Relying on it in production
defeats the whole point.

## Staleness is the important part

The document carries `generated_at` and `refresh_seconds`. If what the page has
is older than a few refresh intervals, it says so plainly instead of showing a
confident green. If it cannot load the document at all, it says the status is
unknown rather than rendering an empty, reassuring page.

## Components

A component either **follows an uptime check** or is **operator-controlled**.

Following a check means its state is an observation. A check that has never
reported reads as operational rather than as an outage — announcing an outage
because a probe has not run yet would be its own kind of false alarm.

Operator-controlled components are for the things no probe covers, and can be
set to `operational`, `degraded`, `partial_outage`, `major_outage` or
`maintenance`.

The banner rolls up conservatively: every component out reads as a major
outage, some out as partial, anything else not fully operational as degraded.

## Incidents

An incident is a title, an impact, and a timeline. Each update moves its status
through `investigating → identified → monitoring → resolved`; the resolving
update stamps `resolved_at` and moves it out of the active list into recent,
where it stays for two weeks.

An unrecognised impact falls back to `minor` rather than alarming people.

## Hosting the page

`monitoring/status/index.html` is one file with no dependencies, no build step
and no framework. Point `SNAPSHOT_URL` at wherever the collector publishes and
serve it from anywhere — ideally not from the deployment it reports on.

## Not built

Email subscribers for incident updates. The page and the document exist; a
subscription list and a double-opt-in flow do not.
