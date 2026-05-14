# Erno — SaaS Framework Roadmap

## In Progress / Decided

- [x] **Remove SQLite support** — PostgreSQL only going forward

---

## Core User Features

- [x] **User model + registration** — out-of-the-box user entity, sign-up endpoint
- [x] **Password reset flow** — full flow (request → email → token verify → update); token generation already exists
- [x] **Email verification flow** — verify email on registration, resend endpoint

---

## Offline-First Sync

### Done
- [x] **`sync_push_queue` table + global sequence** — PostgreSQL `erno_sync_clock` sequence; trigger fires `NOTIFY sync_new_event` on every insert
- [x] **`SyncQueue`** — `push()` API for writing change events; mock variant for tests
- [x] **`Syncable` trait + `SyncRegistry`** — entities declare their associated `Policy`; registry evaluates `policy.can_read()` per connected user
- [x] **`FromUser` trait** — standard way to instantiate a policy from a `user::Model` for use in the sync worker
- [x] **WebSocket push with policy-based filtering** — background listener wakes on NOTIFY, batch-loads connected user models, sends events only to users whose policy permits read access; deletes queue rows after delivery

### Remaining

- [ ] **`add_sync_columns` migration helper** — adds `sync_seq BIGINT` and `deleted_at TIMESTAMP` columns + a per-table trigger to entity tables; the trigger sets `NEW.sync_seq = nextval('erno_sync_clock')` on every insert/update and writes to `sync_push_queue`; required by everything below

- [ ] **Per-entity delta sync endpoint** — handler factory `sync_delta_handler::<E>()` that apps mount at e.g. `/posts/sync`; queries the entity table with `WHERE sync_seq > $since AND <policy::readable()>`; returns `{ items, next_since }`; soft-deleted records (non-null `deleted_at`) are included so clients can remove them locally. **Note:** the current `/api/sync/changes` endpoint queries `sync_push_queue` rows which are now deleted after WS delivery — remove it once per-entity endpoints are in place.

- [ ] **Optimistic concurrency** — `sync_seq` doubles as the concurrency token (no separate `version` column); handler helper `check_sync_version(&entity, client_seq)` returns 409 Conflict with `{ error, server_sync_seq, server_record }` if the client's seq is stale

- [ ] **Soft delete helper** — `app.soft_delete::<E>(&db, id)` sets `deleted_at = now()`, which triggers the sync capture; apps use this instead of SeaORM `.delete()` on syncable entities

- [ ] **Sync conflict surfacing** — structured 409 response body gives the client the current server record so it can present resolution UI without an extra round trip

---

## Deployment

- [ ] **Dockerfile generation** — production-ready multi-stage Dockerfile as part of `erno new` or a `generate` command
- [ ] **Helm chart generation** — Kubernetes Helm chart template for deploying erno apps
- [ ] **Production config guidance** — secrets management, health check wiring (/liveness, /readiness already exist)

---

## File Storage

- [ ] **Active Storage equivalent** — file upload handling, S3/object storage backend, local dev backend, attachment associations

---

## Authentication & Authorization

- [ ] **API keys** — generate/revoke API keys for service-to-service auth, scoped permissions
- [ ] **2FA / MFA** — TOTP-based two-factor authentication
- [ ] **RBAC / roles** — roles and permissions table, role assignment, role-aware policies

---

## Billing & Subscriptions

- [ ] **Stripe integration** — subscription plans, checkout, webhooks (payment events → job queue)
- [ ] **Plan-based feature gates** — middleware/extractor to check subscription tier

---

## Outbound Webhooks

- [ ] **Webhook delivery system** — register endpoints per event type, deliver via job queue, retry on failure, signature verification

---

## Observability & Monitoring

- [ ] **OpenTelemetry integration** — add `tracing-opentelemetry` + `opentelemetry-otlp`; bridge existing `tracing` instrumentation to OTLP export; config chooses backend (Grafana, Datadog, etc.)
- [ ] **Metrics** — expose Prometheus-compatible `/metrics` endpoint (request rates, job queue depth, error rates); exportable via OTel
- [ ] **Structured logging** — ensure logs are JSON-formatted in production for ingestion by Loki/Datadog; human-readable in dev (already partially there)
- [ ] **Audit log** — structured log of who did what and when, queryable, retention policy
- [ ] **Alerting guidance** — document recommended alerts (error rate spike, job queue backlog, slow queries) for Grafana/Datadog

---

## Developer Experience

- [ ] **App scaffolding / generator** — `erno new my_app` to scaffold a new project with config, migrations, user model wired up
- [ ] **Resource generator** — `erno generate resource Post title:string body:text` (Rails-style)
- [ ] **TypeScript type generation** — `ts-rs` derive on API structs + `erno generate typescript` CLI command; outputs type-safe `.ts` interfaces for Angular/Ionic to import; run in CI when models change

---

## Already Solid ✓

- JWT authentication + CurrentUser extractor
- Policy-based authorization (Pundit-style) + `FromUser` trait for policy instantiation
- Argon2 password hashing + secure token generation
- PostgreSQL job queue (LISTEN/NOTIFY, retry, cron, recovery)
- SMTP mailer (mock/real swap)
- Multi-tier adaptive rate limiting
- WebSocket support with JWT auth
- SeaORM migrations + CLI (`migrate up/down/status/reset`)
- Rhai scripting console
- Per-test transaction isolation + mock queue/mailer
- `SyncQueue` + `SyncRegistry` + `Syncable` trait for offline-first sync
- WebSocket push with policy-based recipient filtering
