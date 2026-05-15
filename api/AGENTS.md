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
| `admin` | Adds `ratatui` for the admin TUI |

## Key modules

| Module | Responsibility |
|--------|---------------|
| `auth` | JWT access + refresh tokens, registration, password reset, email verification |
| `sync` | Offline-first delta sync engine (PostgreSQL LISTEN/NOTIFY) |
| `jobs` | Background job queue (PostgreSQL advisory locks + worker pool) |
| `billing` | Stripe integration, trial management |
| `storage` | S3 / local file storage abstraction |
| `rate_limiting` | Multi-tier adaptive rate limiting |
| `policy` | Pundit-style authorization (`Policy` trait) |
| `metrics` | Prometheus metrics |
| `admin` | CLI/TUI admin commands (requires `admin` feature) |

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
| `auth` | `authentication.md` |
| `billing` | `billing.md` |
| `storage` | `storage.md` |
| `sync` | `sync.md` |
| `jobs` | `jobs.md` |
| `rate_limiting` | `rate-limiting.md` |
| `policy` | `authorization.md` |
| `metrics` | `telemetry.md` |
| `admin` | `console.md` |

**If you change a module's public API, configuration keys, or observable behaviour, update the corresponding doc page.**
