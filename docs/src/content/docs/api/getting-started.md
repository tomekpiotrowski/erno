---
title: Manual API setup
description: Add the Erno Rust library without the CLI scaffold
sidebar:
  order: 1
---

> Prefer the full-stack path? See **[Getting started](/getting-started/)** — `erno new` scaffolds API + app + databases for you.

This page covers using the Erno Rust library on its own.

## Installation

Add Erno to your `Cargo.toml`:

```toml
[dependencies]
erno = { git = "https://github.com/tomekpiotrowski/erno", tag = "v0.1.0" }
sea-orm-migration = { version = "1.1", features = ["sqlx-postgres"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
async-trait = "0.1"
axum = "0.8"
```

Erno requires Rust **1.88.0** or later.

## Minimal application

```rust
mod migrations;

use erno::{
    app::App,
    app_info::AppInfo,
    boot::{boot, BootConfig},
    jobs::job_registry::JobRegistry,
};
use axum::{routing::get, Router};
use migrations::Migrator;

async fn health() -> &'static str { "OK" }

fn router(_app: App) -> Router {
    Router::new().route("/health", get(health))
}

fn boot_config() -> BootConfig {
    BootConfig::new(
        AppInfo::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), ""),
        router,
        JobRegistry::new(),
        vec![],
    )
}

#[tokio::main]
async fn main() {
    boot::<Migrator, ()>(boot_config()).await;
}
```

## Migrator

Your `Migrator` runs all framework migrations first, then your own:

```rust
// src/migrations/mod.rs
use erno::database::migrations::erno_migrations;
use sea_orm_migration::{MigrationTrait, MigratorTrait};

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        erno_migrations()
        // chain your own migrations here
    }
}
```

## Configuration

Erno reads `config/{environment}.toml`. The active environment defaults to `development` and can be changed with `APP_ENVIRONMENT`.

```toml
base_url = "http://localhost:3000"

[tracing]
log_level = "info"

[database]
url = "postgres://user:password@localhost/mydb"
pool_size = 5

[server]
port = 3000

[email]
type = "mock"

[auth]
secret = "<random 32+ byte string — generate with: cargo run -- generate-jwt-secret>"
access_token_minutes = 15
one_time_token_expiry_hours = 24
refresh_token_days = 30
```

Environment variables prefixed `APP_` override any TOML value; use `__` for nesting (`APP_DATABASE__URL`).

See [Boot & Configuration](/api/boot/) for the full option reference.

## Built-in CLI commands

Every Erno application exposes these commands via `cargo run --`:

| Command | Description |
|---------|-------------|
| `serve` (default) | Start the HTTP server |
| `db migrate up` | Run pending migrations |
| `db migrate down --steps N` | Roll back N migrations |
| `db migrate status` | Show applied and pending migrations |
| `db migrate reset` | Roll back all, then migrate up |
| `db migrate reapply` | Roll back and reapply recent migrations |
| `db console` | Open a psql session |
| `db reset` | Drop all tables and types, then migrate up |
| `routes` | List all registered routes |
| `generate-jwt-secret` | Print a random secret suitable for `[auth].secret` |
| `version` | Show version and build info |

## Next steps

- [Boot & Configuration](/api/boot/) — full `BootConfig` and `AppState` reference
- [Authentication](/api/authentication/) — protect routes with JWT
- [Jobs](/api/jobs/) — run background tasks
- [Sync](/api/sync/) — offline-first delta synchronization
