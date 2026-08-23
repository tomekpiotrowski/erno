# Erno API

Rust library crate. All commands below run from this directory (`api/`).

## Building & testing

```sh
cargo build --all-features
cargo test --all-features           # requires PostgreSQL — see below
cargo clippy --all-features         # lint
cargo fmt                           # format
cargo doc --open                    # generate + open API docs
```

**Tests require PostgreSQL** at `postgres://erno:erno@localhost/erno` (configured in `config/test.toml`).
Rate limiting and email sending are disabled in the test environment.

## Feature flags

| Flag | Purpose |
|------|---------|
| `test-utils` | Adds `axum-test` + `lets_expect`; needed to compile and run tests |

## Key modules

| Module | Responsibility |
|--------|---------------|
| `auth` | JWT access + refresh tokens, registration, password reset, email verification |
| `sync` | Offline-first delta sync engine (PostgreSQL LISTEN/NOTIFY) |
| `share` | Share entities via secret links or direct grants (`Principal` + `FromPrincipal`), sync-integrated |
| `jobs` | Background job queue (PostgreSQL advisory locks + worker pool) |
| `billing` | Stripe integration, trial management |
| `storage` | S3 / local file storage abstraction |
| `rate_limiting` | Multi-tier adaptive rate limiting |
| `policy` | Pundit-style authorization (`Policy` trait) |
| `metrics` | Prometheus scrape target + `db_stats` gauges |
| `admin` | HTTP admin API (`/admin/api/*`) for the Angular operator app |
| `admin_events` | Operator event log + matching counters |
| `error_reporting` | Error capture and reporting; the collector half runs in the separate `monitoring/` deployment |

## Architecture notes

- **Library crate**: consuming apps boot via `app.rs`; see `examples/simple_api.rs` for a full example
- **Policy-based authz**: implement the `Policy` trait per resource type
- **Background jobs**: implement the `Job` trait, register in `JobRegistry`
- **Config**: TOML files per environment in `config/` (development.toml, test.toml)
- **Minimum Rust version**: 1.88.0

## Documentation

Narrative docs for each module live in `docs/src/content/docs/api/`:

| Module | Doc page |
|--------|---------|
| (boot / config) | `boot.md` |
| (manual setup) | `getting-started.md` (API-only; full-stack is `docs/.../getting-started.md`) |
| `auth` | `authentication.md` |
| `billing` | `billing.md` |
| `storage` | `storage.md` |
| `sync` | `sync.md` |
| `share` | `share.md` |
| `jobs` | `jobs.md` |
| `rate_limiting` | `rate-limiting.md` |
| `policy` | `authorization.md` |
| `metrics` | `telemetry.md` |
| `admin` | `console.md` |
| `admin_events` / gauges | `business-stats.md` |
| `error_reporting` | `../monitoring/error-reporting.md` |
| `websocket` | `websocket.md` |
| `emails` / mailer | `email.md` |
| `database` | `database.md` |

Cross-cutting guides: `docs/src/content/docs/guides/` (e.g. sync end-to-end, billing gates). Architecture overview: `docs/src/content/docs/architecture.md`.

**If you change a module's public API, configuration keys, or observable behaviour, update the corresponding doc page.**
