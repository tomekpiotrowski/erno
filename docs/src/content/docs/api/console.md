---
title: Admin console
description: HTTP admin API and the Angular operator app
sidebar:
  order: 9
---

> **Source**: `api/src/admin/` (server), `admin/` (Angular app)

Erno’s operator surface is an Angular app (`admin/`, port 4300 in development) plus the HTTP API under `/admin/api/*` (Basic auth, Argon2 password hash). The old `erno admin` TUI has been removed.

The SPA talks to `/admin/api` for records and mutations, and to Prometheus (`/prometheus` same-origin proxy) for charts. The API does not proxy PromQL.

## Enable the admin API

```toml
[admin]
username = "admin"
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$..."
```

If `password_hash` is missing or empty, admin routes are not mounted.

Development password is **`admin`**. `erno deploy init` generates a production password once and stores only the hash.

CORS should include the admin origin (`http://localhost:4300` in development).

## API surface

All routes require `Authorization: Basic …` and live under `/admin/api`.

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/dashboard` | User mix, job health, email outbox stats |
| `GET` | `/users?q=&page=&per_page=` | Search users |
| `GET` | `/users/{id}` | Detail, OAuth providers, subscription history |
| `POST` | `/users/{id}/activate` | Mark email verified |
| `DELETE` | `/users/{id}` | Purge user |
| `POST` | `/users/{id}/gift` | Gift `{ "plan", "duration_days" }` |
| `GET` | `/jobs?status=&type=` | Queue stats + list |
| `GET` | `/jobs/{id}` | Arguments + executions |
| `POST` | `/jobs/{id}/retry` | Re-queue |
| `GET` | `/emails?to=&template=&status=` | Outbox metadata |
| `GET` | `/emails/{id}` | One outbox row |
| `GET` | `/tables` | Approximate `n_live_tup` sizes |
| `GET` | `/events?name=&days=` | Business event feed |
| `GET` | `/plans` | Stripe plan names |

There is no `/stats` or `/performance` on the API. Charts query Prometheus.

## Local run

`erno dev` starts the API, Prometheus (if on `PATH`), and the admin SPA when `admin/` is present (project copy or Erno path checkout). Open `http://localhost:4300` (admin / admin).
