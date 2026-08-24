---
title: Architecture
description: How the Erno monorepo pieces fit together — API, app, CLI, and request flows
---

Erno is a **full-stack SaaS framework**: a Rust library for Axum APIs, an Angular client library, and a CLI that scaffolds and deploys projects. You ship product logic; Erno owns auth, jobs, billing, sync, storage, and operator tooling.

## Monorepo layout

| Package | Path | Role |
|---------|------|------|
| **API** | `api/` | Rust library crate — boot, auth, jobs, billing, sync, storage, admin |
| **App** | `app/` | `erno-angular` — Angular 22 services for Ionic web + mobile |
| **CLI** | `cli/` | `erno` binary — setup, doctor, new, upgrade, deploy |
| **Docs** | `docs/` | This Starlight site |

Consuming apps are **not** this monorepo: `erno new` generates a separate project that depends on `erno` (git/path) and `erno-angular` (npm/tarball).

```
  erno CLI  ──scaffolds / deploys──►  your project
                                        ├── api/   (Axum + erno)           → api.product.com
                                        ├── app/   (Ionic + erno-angular)  → app.product.com
                                        └── www/   (Astro static landing)  → product.com
                                              │
                    app ── HTTPS + WebSocket ─┘
                                              ▼
                                           Erno API
                                              │
                         ┌────────────────────┼────────────────────┐
                         ▼                    ▼                    ▼
                   PostgreSQL          S3 or local disk     SMTP or mock mail
```

Marketing (`www/`) is a separate static site for SEO. It does not talk to the API directly; CTAs send users to the product app host for auth and the rest of the product.

## Boot path (API)

1. Your binary calls `boot::<Migrator>(boot_config())`.
2. Erno parses CLI args (`serve`, `db`, …), loads `config/{environment}.toml`, and connects to Postgres.
3. Framework migrations run first (`erno_migrations()`), then your migrator.
4. Job workers, WebSocket/sync listeners, and the Axum router start.

`BootConfig` registers the app router, job registry, schedules, and optional sync entities (`.with_sync` / `.with_sync_shared`). See [Boot & configuration](/api/boot/).

## Auth token flow

1. App → `POST /api/auth/login` → API returns `access_token` + `refresh_token`.
2. Client stores access in `sessionStorage`, refresh in `localStorage`.
3. `ErnoHttpInterceptor` adds `Authorization: Bearer <access>` to every request against `baseUrl`.
4. On `401`, client calls `POST /api/auth/refresh`, stores new tokens, retries the request.
5. Logout / password change increments `token_version` on the user, invalidating old JWTs.

Details: [Authentication (API)](/api/authentication/), [Authentication (App)](/app/authentication/).

## Sync push and pull

Offline-first entities carry `sync_seq` and soft-delete columns.

1. `INSERT` / `UPDATE` / soft `DELETE` fires a PostgreSQL trigger that stamps `sync_seq`.
2. A row is written to `sync_push_queue` and `NOTIFY` wakes the sync listener.
3. The listener evaluates each connected **principal** (user + active shares) against the entity policy and pushes over WebSocket.
4. Offline clients call the delta endpoint with their last `sync_seq` and catch up.

See [Sync (API)](/api/sync/), [Sync (App)](/app/sync/), or the [end-to-end guide](/guides/sync-an-entity/).

## Sharing

Shares grant read access via a secret link token or direct user grants. Authorization widens from “current user” to a **`Principal`** (optional user + active shares). Shared data can be viewed online via `ErnoSharedViewService` without polluting the owner’s offline IndexedDB. See [Sharing (API)](/api/share/) and [Sharing (App)](/app/share/).

## Billing gates

Subscription state is cached on the user row. Handlers take `ActiveSubscription` (402 if none). The client uses `ErnoBillingService` for checkout, portal, and subscription status. See [Billing (API)](/api/billing/) and [Gate features with billing](/guides/billing-gates/).

## Jobs and email

Background work uses a PostgreSQL-backed queue (advisory locks, workers, cron). Email goes through `app.mailer` (SMTP or mock). In development, mock mail is browsable via [Devtools](/app/devtools/).

## Ops surface

| Surface | Purpose |
|---------|---------|
| `/admin/api/*` + `admin/` SPA | Users, gifts, jobs, emails (Basic auth). Charts via Prometheus. |
| Business stats snapshots | Daily SaaS metrics for the TUI |
| `/metrics` | Prometheus |
| `erno deploy` | Docker/Kubernetes scaffold and install |

See [Admin console](/api/console/), [Business stats](/api/business-stats/), and [Deploy](/cli/deploy/).
