---
title: Error reporting
description: Self-hosted error capture, grouping and triage for the API, Angular apps, and the admin console.
sidebar:
  order: 1
---

> **Source**: `api/src/error_reporting/`, `monitoring/`

Reports arrive from every Erno component, are grouped into **issues** by a
server-computed fingerprint, and are triaged in the monitoring console.

## Issues and events

An **event** is one occurrence. An **issue** is every event that shares a
fingerprint, with a lifetime count, first/last seen, first/last release, and a
triage status.

Two numbers on the detail screen are deliberately different:

- **counted** (`times_seen`) — every occurrence reported.
- **stored** — how many event rows actually exist.

They diverge because of the per-flush burst cap, and because reports are shed
when the collector is saturated. `times_seen` is a **floor**, never an
overstatement.

## The ingest endpoint

`POST /api/errors` on the monitoring deployment.

```jsonc
{
  "events": [
    {
      "type": "TypeError",              // required
      "message": "x is not a function", // required
      "level": "error",                 // warning | error | fatal
      "timestamp": "2026-08-22T10:00:00Z",
      "stack": "TypeError: …\n    at Foo (…)",
      "frames": [{ "function": "Foo", "file": "…", "line": 12, "column": 5 }],
      "fingerprint": ["custom", "key"], // optional grouping override
      "context": { "url": "…", "route": "/decks/:id", "user_agent": "…", "trace_id": "…" },
      "user": { "id": "…", "email": "…" }  // trusted callers only
    }
  ],
  "release": "1.4.2",
  "environment": "production",
  "sdk": { "name": "erno-angular", "version": "0.1.0" }
}
```

`source` is **never** taken from the wire — the collector assigns it from the
credential presented. When a request span is active, the reporter also sets
`context.trace_id` so the Issues page can open the
[Tempo waterfall](/monitoring/tracing/).

### Oversized input is truncated, not rejected

Messages, stacks, frame counts and context blobs are all capped, and anything
over the cap is trimmed. The response is always `202` on an authenticated,
parseable request, even when every report was shed.

This is deliberate. A reporter that receives a `4xx` retries for ever, so
rejecting a payload turns one bad report into a permanent hot loop against the
collector. Only JSON that will not deserialise at all is refused, with `422`.

| Cap | Value |
|---|---|
| Events per request | 20 |
| Message | 4 KiB |
| Stack | 16 KiB |
| Frames | 50 |
| Context | 8 KiB serialised |
| Request body | 64 KiB |
| Clock skew | ±5 minutes, then clamped |

## Authentication

Two tokens **per project**, stored as SHA-256 hex on the project row. Lookup is
by hash (server column first, then browser). Empty stored hashes never match.
Plaintext is shown once at create and rotate.

| Caller | Header | Trust |
|---|---|---|
| Server-to-server | `X-Erno-Ingest-Key: <project server token>` | Trusted — may attribute a user, and is the `api` source |
| Browsers | `X-Erno-Ingest-Key: <project browser token>` | Untrusted — attribution discarded, source forced to `app` or `admin` |

**The browser token is public.** It ships inside the JavaScript bundle, and
anyone who opens devtools can read it. It is a speed bump against drive-by
scanners and misdirected traffic, not a security control. What actually bounds
abuse is the rate limiting, the bounded queue, the burst cap and retention.

Two separate tokens mean the public one can be rotated without redeploying the
API, and a leaked public token can never impersonate a server. A Teryon token
cannot ingest as Cubeast. **TLS is mandatory** — user emails cross the internet
on this path.

Create a project with `POST /api/collector/projects` (operator Basic) or the
boot seed: if `project` is empty, the collector inserts slug `monitoring` from
`[error_reporting] ingest_token` (and optional `[collector.seed]`).

Browsers are cross-origin. The collector CORS layer is the union of
`project.cors_origins` and `[cors] allowed_origins` (the extras: console
origin). A missing origin fails *silently*: reports simply stop.

## Rate limiting

Two layers, because the path-based middleware runs before any authentication
and so can only ever limit by IP.

| Action | Applies to | Tiers |
|---|---|---|
| `error_ingest` | Per IP, identity-blind ceiling | 300/10s · 1500/60s · 15000/h |
| `error_ingest_server` | `token:server:{project_id}` | 100/10s · 600/60s · 10000/h |
| `error_ingest_browser` | `ip:{ip}:{project_id}` | 10/10s · 30/60s · 200/h |
| `otlp_auth` | nginx `auth_request` for Tempo/Loki ingest | **exempt** — all pushes share the console pod's IP |

The browser tier is loose on purpose. A corporate NAT or a university campus
puts hundreds of real users behind one IPv4, so a tight limit would blackhole a
whole office's crash reports rather than stop an abuser. Including `project_id`
in the bucket so a leaked Teryon browser token cannot spend Cubeast's quota.

The limiter is an in-memory map **per process**: with *N* collector replicas the
effective quota is *N ×* the configured number, and a rolling deploy resets
every bucket.

## Fingerprinting

Computed **only at the collector** — a client bug is exactly what produces bad
grouping, so clients are never trusted with it.

The key is `sha256` over, in order:

1. **Project id**, always. Two apps with the same stack cannot merge.
2. **Source**, always. A browser `TypeError` and a Rust panic can never merge.
3. A client-supplied `fingerprint`, if present — then stop.
4. The normalised exception type.
5. The top 5 stack frames as `function@file`, preferring frames that are not
   in `node_modules`, the cargo registry, or the standard library.
6. If there is no stack at all: the call site plus a normalised message, with
   UUIDs, numbers, quoted strings, URLs and emails replaced by placeholders.

### Line numbers are excluded

They are stored on the event and shown in the UI, but they are **not** part of
the key. Including them would mint a brand-new issue on every deploy that
touched the file — the most common way homegrown groupers fail.

Also normalised away: bundler content hashes (`main-A1B2C3D4.js` → `main.js`),
Rust symbol hashes and closure suffixes, generic arguments, and cargo registry
versions (`sqlx-core-0.8.2/…` → `sqlx-core/…`).

### Why did these two errors group / not group?

- **Grouped, and you did not expect it** — check whether they share their top
  five in-app frames, or whether both are stackless and their messages
  normalise to the same text.
- **Not grouped, and you expected it** — the usual cause is a different stack
  reaching the same function. Use an explicit `fingerprint` to force them
  together.

## Regression detection

A **resolved** issue that occurs again after it was resolved flips back to
`unresolved` and clears `resolved_at`. An **ignored** issue stays ignored, no
matter how often it recurs — that is what ignoring means.

Resolution timestamps are written in UTC. Anything setting `resolved_at` by
hand must do the same, or a recurrence will silently never reopen the issue.

## What is collected, and what is scrubbed

Collected: the error's type, message and stack; the page URL; the user agent;
release and environment; and, when the reporter knows one, the user's id and
email.

Never collected: cookies, `localStorage` or `sessionStorage`, request bodies,
response bodies, or form values.

Scrubbing runs **twice** — in `errors/scrub.ts` before anything leaves the
device, and in `collector/scrub.rs` before anything is stored. Neither is
redundant: the client pass is the only thing that can stop a secret crossing
the network to another provider; the server pass is the only thing that can
stop it being stored, and it is the only pass at all for `source = api`.

| Rule | Effect |
|---|---|
| Key denylist | `auth`, `token`, `password`, `secret`, `apikey`, `key`, `cookie`, `session`, `csrf`, `signature`, `card`, `cvv`, `ssn`, … → `[redacted]` |
| Key matching | Tokenised on word and camelCase boundaries, so `author` is not mistaken for `auth` |
| JWTs, `Bearer`/`Basic`, card numbers | `[redacted]` |
| Opaque runs ≥32 chars | `[redacted]` |
| UUIDs | **Kept** — identifiers, not credentials, and usually the detail that makes an error traceable |
| URL fragment | **Dropped entirely** — Erno share links carry their secret there |
| URL query | Denylisted parameters redacted, the rest kept |
| Depth | Recursion stops at 5 levels |

Client IPs are **not** stored unless `store_client_ip = true`.

### Account deletion

An application's deletion path calls
`DELETE /api/collector/users/{id}/events` with the trusted token. The collector
nulls `user_id` and `user_email` on matching events but **keeps the rows**: a
stack, a release and a grouping are not personal data, and deleting them would
corrupt `times_seen` and every chart already drawn.

## Capturing the API's own errors

| Source | Mechanism | Default |
|---|---|---|
| `tracing::error!` | A `tracing` layer on the subscriber registry | on |
| Panics in a request | `CatchPanicLayer` → clean 500, reported by the hook below | on |
| Panics anywhere else | `std::panic::set_hook`, chained to the previous hook | on |
| Failed background jobs | Covered by the tracing layer, which sees the existing error log | on |
| 5xx responses | Not implemented (`capture_5xx` reserved) | off |

Three structural duplicates are suppressed so one panic produces one issue:
the reporting subsystem's own target, `tower_http::catch_panic`, and
`tower_http::trace::on_failure`. Panic backtraces also have the unwinding
machinery stripped, so the culprit names the code that panicked rather than the
reporter — without that, every panic would share top frames and collapse into a
single issue.

### Day-one noise

Turning on `capture_tracing_errors` makes every pre-existing `tracing::error!`
into an issue. Production templates ship with it **off**: enable it in staging
first, triage, add `ignore_targets` entries for anything chatty, then enable it
in production.

## The reporter cannot hurt the application

Reporting now crosses the network, so the application-side reporter is built
around one rule: **it must never be able to hurt the application it reports
for.**

- `capture()` is synchronous, non-blocking and infallible — a `tracing` layer
  cannot await, and a panic hook may not even be on the runtime.
- The queue is bounded; a full queue sheds the newest report and counts it.
- Every request has a hard timeout (default 5s).
- Retries use exponential backoff with jitter, capped at 60s.
- A **circuit breaker** opens after 5 consecutive failures and stays open for a
  minute. Without it, an unreachable collector means a retry storm and a pool
  of hanging requests — the reporter becoming the outage.
- A 4xx other than 429 discards the batch instead of retrying it.
- Reports lost while the collector is unreachable are lost. There is no disk
  spill, because that would mean another store in the application deployment,
  which is the thing being avoided.

## Alerting

The first time a fingerprint is seen, the operator gets an email. There is no
alert for later occurrences — recurrence is visible in the console, and mailing
on every occurrence is how a monitoring tool trains a team to filter it out.

`alert_sent_at` on the issue makes this idempotent across restarts and
replicas: a restarted writer will not re-alert an issue that already went out.

Three limits stack, because a bad deploy can mint hundreds of fingerprints in
a minute:

| Limit | Default | Effect |
|---|---|---|
| `max_per_window` | 10 | Individual emails per window |
| `window_minutes` | 60 | Length of the window |
| `min_interval_seconds` | 30 | Floor on the gap between two emails |

Past the cap, alerts are held and a single digest goes out when the window
rolls: *"N additional new error types were suppressed."* The worst case for a
catastrophic deploy is therefore **cap + 1 emails per hour**, ever.

`recipient` defaults to empty, which leaves alerting inert until an operator
configures it. Together with `capture_tracing_errors` shipping off in
production, that is what stops a first deploy mailing fifty times about
pre-existing error logs.

Alerting runs in its own task. SMTP is slow, and the write loop must never wait
on it; a full alert queue drops the alert rather than the reports. Mail is sent
directly rather than through the job queue — registering a built-in job type
would panic every existing deployment at boot until its worker pools were
edited, which is far too high a price for an email. The tradeoff is no retry: a
failed alert is counted and dropped.

## Retention

A sweep runs hourly under a deployment-wide advisory lock, so replicas do not
duplicate it:

1. Events older than `event_retention_days` (default 30) are deleted.
2. Any issue holding more than `max_events_per_issue` (default 500) is trimmed
   to its newest.
3. Issues whose `last_seen` is older than `issue_retention_days` (default 90)
   are deleted; their events cascade.

Every delete is batched through an `id IN (SELECT … LIMIT n)` subselect, so no
statement holds a long lock on a table ingest is actively writing to.

`times_seen` is **never** decremented by retention. It is a lifetime counter,
and a number that shrank as rows were pruned would be baffling in the console —
which is why the detail screen shows *counted* and *stored* separately.

## Operator API

Under `/api/collector`, HTTP Basic.

| Method | Path |
|---|---|
| `GET` | `/issues` — `status`, `source`, `q`, `release`, `hours`, `page`, `per_page` |
| `GET` | `/issues/counts` |
| `GET` | `/issues/{id}` |
| `GET` | `/issues/{id}/events` |
| `GET` | `/issues/{id}/series` · `/series` |
| `POST` | `/issues/{id}/resolve` · `/ignore` · `/unresolve` |
| `DELETE` | `/issues/{id}` — cascades to events |
| `DELETE` | `/users/{id}/events` — trusted token, not operator credentials |

`per_page` is clamped to 200 and `hours` to one year. Search is a case-insensitive
substring over title, type and culprit, with `%` and `_` escaped.

Time series are bucketed server-side — minute below 6 hours, hour below 48,
day beyond — and **zero-filled**, because a sparkline that omits empty buckets
draws a flat line through an outage and lies about it.

## Volume control

Layered, because no single limit is sufficient:

1. Client dedupe window and per-minute cap.
2. Client sample rate.
3. Reporter circuit breaker.
4. Tiered ingest rate limits.
5. Events-per-request cap and body limit.
6. Bounded queue, drop-newest.
7. **Per-flush per-fingerprint burst cap** — the most effective one. A loop
   firing 10k/s costs 10 rows per flush while every occurrence is still counted.
8. Retention and the per-issue event cap.
9. `enabled = false`, which unmounts the route entirely.

## Configuration

### Application side

```toml
[error_reporting]
enabled = true
collector_url = "http://localhost:3001"   # empty = reporting off
ingest_token = ""                          # server token; a real secret
queue_capacity = 1024
batch_size = 200
flush_interval_ms = 1000
request_timeout_ms = 5000
circuit_breaker_failures = 5
circuit_breaker_cooldown_ms = 60000
capture_tracing_errors = true
capture_panics = true
capture_5xx = false                        # reserved
ignore_targets = []
```

### Monitoring side

```toml
[collector]
enabled = true
sync_writes = false        # true in tests
queue_capacity = 1024
batch_size = 200
flush_interval_ms = 1000
max_events_per_request = 20
max_events_per_flush_per_issue = 10
max_body_bytes = 65536
store_client_ip = false
event_retention_days = 30
issue_retention_days = 90
max_events_per_issue = 500

# Used only when the project table is empty. Ignored after that.
[collector.seed]
server_token = ""
browser_token = ""

[collector.status]
output_path = "status/"    # a directory; `{dir}/status.json` until snapshots are project-scoped

[collector.alerts]
enabled = true
recipient = ""             # empty leaves alerting inert; org-level only
min_level = "error"
max_per_window = 10
window_minutes = 60
min_interval_seconds = 30
```

Every key has a default, so an existing config file that has never heard of
error reporting keeps booting unchanged. Environment overrides work as
elsewhere: `APP__ERROR_REPORTING__INGEST_TOKEN`.

## Metrics

| Metric | Meaning |
|---|---|
| `erno_error_reports_received_total{source}` | Accepted at ingest |
| `erno_error_reports_written_total` | Event rows written |
| `erno_error_reports_sent_total` | Delivered by an application's reporter |
| `erno_error_reports_dropped_total{reason}` | `queue_full`, `burst_cap`, `per_request_cap`, `unreachable`, `circuit_open`, `rejected`, `closed` |
| `erno_error_report_write_failures_total` | Collector write failures |
| `erno_error_issues_created_total` | New fingerprints |

If `erno_error_reports_dropped_total` is non-zero, `times_seen` is an
undercount — that counter is the ground truth for how much was lost.
