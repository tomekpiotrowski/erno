---
title: Getting Started
description: Install Erno and build your first Axum application
sidebar:
  order: 1
---

## Installation

Add Erno to your `Cargo.toml`:

```toml
[dependencies]
erno = { git = "https://github.com/tomekpiotrowski/erno" }
```

Erno requires Rust **1.88.0** or later and Tokio as the async runtime.

## Minimal application

```rust
use erno::prelude::*;

#[tokio::main]
async fn main() {
    boot::<Migrator>(boot_config()).await;
}

pub fn boot_config() -> BootConfig {
    let app_info = AppInfo::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_DESCRIPTION"),
    );

    BootConfig::new(app_info, router, job_registry(), job_schedule())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .with_state(state)
}

fn job_registry() -> JobRegistry {
    JobRegistry::new()
}

fn job_schedule() -> Vec<Box<dyn ScheduledJob>> {
    vec![]
}
```

`boot::<Migrator>` runs pending database migrations and then starts the Axum HTTP server. The `Migrator` type comes from your own SeaORM migration crate.

## Configuration

Erno reads configuration from a TOML file named `config/{environment}.toml` (e.g. `config/development.toml`). The active environment defaults to `development` and can be changed with the `APP_ENVIRONMENT` variable.

A minimal config file looks like:

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
secret = "<random 32+ byte string>"
one_time_token_expiry_hours = 24
```

Environment variables prefixed with `APP_` override any TOML value. Use double underscores for nesting (e.g. `APP_DATABASE__URL` overrides `database.url`).

See [Boot & Configuration](../boot) for the full list of options.

## Next steps

- [Boot & Configuration](../boot) — understand `BootConfig` and `AppState`
- [Authentication](../authentication) — protect your routes with JWT
- [Jobs](../jobs) — run background tasks
