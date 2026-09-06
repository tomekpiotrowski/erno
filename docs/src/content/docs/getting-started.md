---
title: Getting started
description: Create a full-stack Erno project and run it locally
---

The fastest path to a working Erno app is the CLI: it scaffolds a Rust API, an Ionic/Angular product app, an Astro marketing site, databases, and framework migrations in one command.

## Prerequisites

- Rust **1.88.0** or later
- Node.js **22.22.3** or later (or 24.15+, or 26+)
- Bun **1.4.0** for dependency installation and package scripts
- Angular CLI (`ng`) and Ionic CLI (`ionic`)
- PostgreSQL (server + `psql` client)
- Optional: `sea-orm-cli` for generating migrations

## 1. Install the CLI

```sh
cargo install erno-cli --git https://github.com/tomekpiotrowski/erno --locked
# or, from a clone of this repo:
cargo install --path cli
```

That installs from `main`. A specific release is `--tag` from the
[latest GitHub Release](https://github.com/tomekpiotrowski/erno/releases/latest).

## 2. Configure your machine

```sh
erno setup    # writes ~/.erno/config.toml (PostgreSQL admin URL)
erno doctor   # verifies Rust, Node, Bun, Postgres, and config
```

The PostgreSQL admin user must be able to create databases:

```sql
CREATE USER erno WITH PASSWORD 'erno';
ALTER USER erno CREATEDB;
```

See the [CLI overview](/cli/) for details.

## 3. Scaffold a project

```sh
erno new my_app
cd my_app
```

This creates:

```
my_app/
├── api/     # Rust backend (erno library)
├── app/     # Ionic / Angular / Capacitor product app (erno-angular)
└── www/     # Astro static marketing / landing page
```

It also creates `my_app_development` and `my_app_test` databases.

The API applies pending migrations on boot. `erno new` can start all three dev servers via `erno dev` (it asks on a TTY; pass `--dev` or `--no-dev` to skip the prompt):

| Surface | URL |
|---------|-----|
| Marketing | http://localhost:4321 |
| Product app | http://localhost:4200 |
| API | http://localhost:3000 |

When developing against a local erno checkout:

```sh
erno new my_app --erno-path /path/to/erno
```

## 4. Run the tests

```sh
erno test
```

Creates the test database if needed, then runs API request specs, the app Karma suite, and Playwright e2e tests (`e2e/`). See [Testing](/api/testing/).

## 5. Run the API

```sh
cd api
cargo run
```

`serve` (the default) waits for migrations before accepting traffic. Use `cargo run -- db migrate up` only when you want to apply migrations without starting the server.

Health check: `http://localhost:3000/health`.

Built-in app commands (`cargo run --` from `api/`):

| Command | Description |
|---------|-------------|
| `serve` (default) | Start the HTTP server |
| `db migrate up` | Run pending migrations |
| `db migrate down --steps N` | Roll back N migrations |
| `db migrate status` | Show applied and pending migrations |
| `db migrate reset` | Roll back all, then migrate up |
| `db migrate reapply` | Roll back and reapply recent migrations |
| `db console` | Open a `psql` session |
| `db reset` | Drop all tables and types, then migrate up |
| `routes` | List all registered routes |
| `generate-jwt-secret` | Print a random secret for `[auth].secret` |
| `version` | Show version and build info |

## 6. Run everything

From the project root:

```sh
erno dev
```

Or start pieces separately:

```sh
# Product app
cd app && bun install && bun run start

# Marketing site
cd www && bun install && bun run dev
```

The app scaffold wires `ErnoModule.forRoot()` with login, register, password reset, and email verification screens. See [App overview](/app/) for service setup.

The `www/` site is a static Astro landing page (SEO-friendly). CTAs link to the product app (`/login`, `/register`). In production those hosts are typically `example.com` (www) and `app.example.com` (app) — see [Deploy](/cli/deploy/).

## What you get

| Layer | Included |
|-------|----------|
| Auth | JWT access + refresh, register, verify email, password reset |
| Jobs | Background queue on PostgreSQL (no Redis) |
| Sync | Offline-first delta sync + WebSocket push |
| Billing | Stripe, gift, and trial subscriptions |
| Storage | Local or S3-compatible files |
| Ops | Admin HTTP API, Angular operator app, Prometheus metrics |

## Manual API-only setup

If you only need the Rust library without the CLI scaffold, see [Manual API setup](/api/getting-started/).

## Next steps

- [Architecture](/architecture/) — how API, app, and CLI fit together
- [Authentication (API)](/api/authentication/) — protect handlers with `CurrentUser`
- [Authentication (App)](/app/authentication/) — login and token refresh on the client
- [Sync an entity end-to-end](/guides/sync-an-entity/) — first offline-first model
- [Deploy](/cli/deploy/) — Docker, Kubernetes, and production install
