# Erno

[![CI](https://github.com/tomekpiotrowski/erno/actions/workflows/ci.yml/badge.svg)](https://github.com/tomekpiotrowski/erno/actions/workflows/ci.yml)

A framework for a SaaS product: Rust/Axum on the server, Angular/Ionic on the client, PostgreSQL underneath. The CLI scaffolds the project and runs it. The libraries cover auth, jobs, billing, offline sync, and files, so application code is mostly product logic.

This repository is the framework. `erno new` creates a *separate* product repo that depends on it.

## Start a project

You need Rust 1.88+, Node 22+, PostgreSQL, and the Angular and Ionic CLIs. `erno doctor` reports anything missing.

```sh
cargo install erno-cli --git https://github.com/tomekpiotrowski/erno --tag v0.2.0 --locked
# or, from a clone:
# cargo install --path cli

erno setup     # PostgreSQL admin URL → ~/.erno/config.toml
erno doctor
```

The Postgres user in that config must be allowed to create databases:

```sql
CREATE USER erno WITH PASSWORD 'erno';
ALTER USER erno CREATEDB;
```

Then, outside this repo:

```sh
erno new my_app
cd my_app
erno dev
```

That writes `api/`, `app/`, and `www/`, creates `my_app_development` and `my_app_test`, and starts the dev servers. A verified demo user (`dev@example.com` / `password`) is seeded on first run. `erno new` can start `erno dev` itself — it asks on a TTY; `--dev` / `--no-dev` skip the prompt.

| Surface | URL |
|---|---|
| Marketing | http://localhost:4321 |
| Product app | http://localhost:4200 |
| API | http://localhost:3000 |

To point the new project at this checkout instead of git (and copy `admin/` in):

```sh
erno new my_app --erno-path /path/to/erno
```

`erno dev` then also serves the operator console at http://localhost:4300 (password `admin`). Telemetry backends are the collector's, and live in the erno-monitoring repository.

Docs: [getting started](docs/src/content/docs/getting-started.md) and the [CLI overview](docs/src/content/docs/cli/index.md). To read them locally, `cd docs && npm install && npm run dev`.

## What you get

| Area | |
|---|---|
| Auth | JWT access + refresh, register, email verification, password reset |
| Jobs | PostgreSQL queue and cron. No Redis. |
| Sync | Offline-first: IndexedDB on the client, delta + WebSocket on the server |
| Billing | Stripe subscriptions, trials, gifts |
| Storage | Local disk or S3 |
| Sharing | Secret links and grants, wired into sync |
| Ops | Admin SPA, Prometheus `/metrics`, a monitoring stack that deploys separately |

The API is the `erno` crate. The client is `erno-angular`, consumed by an Ionic app for web and mobile. `www/` is a static Astro site for SEO; it does not talk to the API. CTAs send people to the product app.

A generated API boots in a few lines. `boot` loads config, runs migrations, and starts the server, workers, and listeners:

```rust
use my_app::{boot_config, Migrator};
use erno::boot::boot;

#[tokio::main]
async fn main() {
    boot::<Migrator, ()>(boot_config()).await;
}
```

```rust
pub fn boot_config() -> BootConfig {
    BootConfig::new(
        AppInfo::new("my_app", env!("CARGO_PKG_VERSION"), ""),
        router,
        JobRegistry::new(),
        vec![],
    )
}
```

Product routes go on that `router`. Syncable entities are `.with_sync::<E>()` on the `BootConfig`. The crate can also be added by hand (`erno = { git = "https://github.com/tomekpiotrowski/erno", tag = "v0.2.0" }`); see [manual API setup](docs/src/content/docs/api/getting-started.md). A new full-stack project should use `erno new`.

## CLI

| Command | |
|---|---|
| `erno setup` | Write `~/.erno/config.toml` |
| `erno doctor` | Check the local environment |
| `erno new <name>` | Scaffold a product repo |
| `erno dev` | Run the API, app, www, and whatever else is present |
| `erno test` / `build` / `lint` | Drive every package from `erno.toml` |
| `erno clean` | Reset local build artifacts and databases |
| `erno upgrade` | Update Erno-managed packages |
| `erno deploy` | Docker + Kubernetes install (no Helm) |

`erno test` creates the test database if needed, runs the API request specs and the app unit tests, then Playwright (`e2e/`).

## This repository

| Directory | |
|---|---|
| `api/` | Rust library crate (`erno`) |
| `app/` | Angular library (`erno-angular`) |
| `cli/` | `erno` binary |
| `admin/` | Operator SPA |
| `error-reporting-types/` | The error-reporting contract this repo shares with the collector |
| `docs/` | Astro documentation site |

`api/`, `cli/`, and `error-reporting-types/` are one Cargo workspace. `app/`, `admin/`, and `docs/` are npm projects. From the repo root:

```sh
./build.sh              # api, cli, app, admin, docs
./build.sh api cli      # a subset
./build.sh test         # Rust suites (need PostgreSQL)
./build.sh check        # fmt + clippy -D warnings
./build.sh help
```

API tests use `postgres://erno:erno@localhost/erno`. The collector lives in its own repository (`erno-monitoring`) with its own database and test suite.

Contributor notes for each part live in [AGENTS.md](AGENTS.md).

## Releases

Cut a version from GitHub: **Actions → Release → Run workflow**, pick `patch` / `minor` / `major`, run it on `main`. The workflow runs CI, bumps the version, tags, and attaches the `erno-angular` tarball to a GitHub Release. From `v0.1.0`, `minor` is `v0.2.0`.

## License

MIT. See [LICENSE](LICENSE).
