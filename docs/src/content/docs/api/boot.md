---
title: Boot & Configuration
description: Application bootstrap, BootConfig, AppState, and environment configuration
sidebar:
  order: 2
---

> **Source**: `api/src/boot.rs`, `api/src/app.rs`

## BootConfig

`BootConfig` is the central struct that wires together all application components. Build one and pass it to `boot`.

```rust
use erno::prelude::*;

pub fn boot_config() -> BootConfig {
    let app_info = AppInfo::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_DESCRIPTION"),
    );

    BootConfig::new(app_info, router, job_registry(), job_schedule())
}
```

| Field | Type | Description |
|-------|------|-------------|
| `app_info` | `AppInfo` | Name, version, and description shown in CLI output |
| `app_router` | `fn(App<ExtraConfig>) -> Router` | Function that builds the Axum router |
| `job_registry` | `JobRegistry` | Map of job names to executor functions |
| `job_schedule` | `Vec<ScheduledJob>` | Cron-based job schedules |

### Registering syncable entities

Use `.with_sync::<E>()` to register an entity for offline-first synchronization. See [Sync](../sync) for the full setup.

```rust
BootConfig::new(app_info, router, job_registry(), job_schedule())
    .with_sync::<post::Entity>()
    .with_sync::<comment::Entity>()
```

### Extra config

`BootConfig` is generic over an optional `ExtraConfig` type. Use it to pass application-specific configuration alongside Erno's built-in config:

```rust
#[derive(Clone, Default, Deserialize)]
pub struct MyConfig {
    pub feature_flag: bool,
}

pub fn boot_config() -> BootConfig<MyConfig> {
    BootConfig::new(app_info, router, job_registry(), job_schedule())
}
```

The extra config is deserialized from the same environment/file source as the rest of the config using serde's `flatten`.

## The `boot` function

```rust
pub async fn boot<AppMigrator: MigratorTrait, ExtraConfig>(config: BootConfig<ExtraConfig>)
```

`boot` does the following in order:
1. Parses the CLI command (uses [clap](https://crates.io/crates/clap) internally)
2. Reads the environment (`APP_ENVIRONMENT` variable, defaults to `development`)
3. Loads configuration from `config/default.toml` and `config/{environment}.toml`
4. Runs `AppMigrator` migrations against the database
5. Starts the Axum HTTP server on the configured port

## Environment

The active environment is set via the `APP_ENVIRONMENT` environment variable. Typical values: `development`, `staging`, `production`.

Config files are loaded in this order (later files override earlier ones):

```
config/default.toml
config/{APP_ENVIRONMENT}.toml
```

## Configuration reference

The full `Config` struct and its fields:

```toml
[server]
port = 3000

[database]
url = "postgres://user:password@localhost/mydb"

[auth]
secret = "<random 32+ byte string>"
access_token_minutes = 15
refresh_token_days = 30
one_time_token_expiry_hours = 24

[tracing]
log_level = "info"

[rate_limiting]
# see Rate Limiting guide

[email]
type = "mock"  # or "smtp"

base_url = "http://localhost:3000"

[metrics]
enabled = true
path = "/metrics"
# auth_token = "secret"
```

## AppState / App

Inside route handlers and jobs, application state is accessed via `App<ExtraConfig>`:

```rust
fn router(app: App) -> Router {
    Router::new()
        .route("/users", get(list_users))
        .with_state(app)
}

async fn list_users(State(app): State<App>) -> impl IntoResponse {
    let users = user::Entity::find().all(&app.db).await?;
    Json(users)
}
```

Key fields on `App`:

| Field | Type | Description |
|-------|------|-------------|
| `db` | `DatabaseConnection` | SeaORM connection pool |
| `config` | `Config<ExtraConfig>` | Full parsed configuration |
| `mailer` | `Mailer` | Email sending service |
| `storage` | `FileStorage` | File storage — local, S3, or mock (see [File Storage](../storage)) |
| `job_queue` | `JobQueue` | Enqueue background jobs |
| `websocket_connections` | `Connections` | Broadcast to authenticated WebSocket clients |
| `sync_queue` | `SyncQueue` | Internal sync event queue |
| `sync_registry` | `Arc<SyncRegistry>` | Registry of syncable entities |
| `metrics_collectors` | `Arc<CollectorRegistry>` | Custom Prometheus metric collectors |
| `prometheus_handle` | `PrometheusHandle` | Handle to the Prometheus recorder |
