---
title: Getting started
description: Create a full-stack Erno project and run it locally
---

The fastest path to a working Erno app is the CLI: it scaffolds a Rust API, an Ionic/Angular frontend, databases, and framework migrations in one command.

## Prerequisites

- Rust **1.88.0** or later
- Node.js and npm
- Angular CLI (`ng`) and Ionic CLI (`ionic`)
- PostgreSQL (server + `psql` client)
- Optional: `sea-orm-cli` for generating migrations

## 1. Install the CLI

```sh
cargo install --path cli      # from a clone of the erno repo
# or, once published:
cargo install erno-cli
```

## 2. Configure your machine

```sh
erno setup    # writes ~/.erno/config.toml (PostgreSQL admin URL)
erno doctor   # verifies Rust, Node, Postgres, and config
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
└── app/     # Ionic / Angular / Capacitor frontend (erno-angular)
```

It also creates `my_app_development` and `my_app_test` databases.

When developing against a local erno checkout:

```sh
erno new my_app --erno-path /path/to/erno
```

## 4. Run the API

```sh
cd api
cargo run -- migrate up
cargo run
```

Health check: `http://localhost:3000/health`.

Built-in app commands (`cargo run --` from `api/`):

| Command | Description |
|---------|-------------|
| `serve` (default) | Start the HTTP server |
| `migrate up` | Run pending migrations |
| `migrate down --steps N` | Roll back N migrations |
| `migrate status` | Show applied and pending migrations |
| `migrate reset` | Roll back all, then migrate up |
| `db console` | Open a `psql` session |
| `db reset` | Drop and recreate the database |
| `routes` | List all registered routes |
| `generate-jwt-secret` | Print a random secret for `[auth].secret` |
| `version` | Show version and build info |

## 5. Run the app

```sh
cd app
npm install
ionic serve
```

The scaffold wires `ErnoModule.forRoot()` with login, register, password reset, and email verification screens. See [App overview](/app/) for service setup.

## What you get

| Layer | Included |
|-------|----------|
| Auth | JWT access + refresh, register, verify email, password reset |
| Jobs | Background queue on PostgreSQL (no Redis) |
| Sync | Offline-first delta sync + WebSocket push |
| Billing | Stripe, gift, and trial subscriptions |
| Storage | Local or S3-compatible files |
| Ops | Admin HTTP API, `erno admin` TUI, Prometheus metrics |

## Manual API-only setup

If you only need the Rust library without the CLI scaffold, see [Manual API setup](/api/getting-started/).

## Next steps

- [Architecture](/architecture/) — how API, app, and CLI fit together
- [Authentication (API)](/api/authentication/) — protect handlers with `CurrentUser`
- [Authentication (App)](/app/authentication/) — login and token refresh on the client
- [Sync an entity end-to-end](/guides/sync-an-entity/) — first offline-first model
- [Deploy](/cli/deploy/) — Docker, Helm, and production install
