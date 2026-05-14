# Erno

[![CI](https://github.com/yourusername/erno/workflows/CI/badge.svg)](https://github.com/yourusername/erno/actions)
[![Crates.io](https://img.shields.io/crates/v/erno.svg)](https://crates.io/crates/erno)
[![Documentation](https://docs.rs/erno/badge.svg)](https://docs.rs/erno)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Full-stack SaaS framework — a Rust/Axum backend library paired with an Angular library for building Ionic web and mobile apps.

## Monorepo layout

| Directory | What it is |
|-----------|------------|
| `api/` | Rust library crate — batteries-included backend (auth, jobs, billing, sync, storage) |
| `app/` | Angular library (`erno-angular`) — consumed by Ionic apps for web and mobile |
| `docs/` | Astro documentation site |

## Features

### Backend (Rust / `api/`)

- **Authentication** - JWT access + refresh tokens, registration, password reset, email verification
- **Offline-first sync** - Delta sync engine with PostgreSQL LISTEN/NOTIFY, conflict detection, soft deletes
- **Job processing** - Background job queue with advisory locks, retry, cron scheduling
- **Billing** - Stripe integration, subscription plans, trial management, plan-based feature gates
- **Storage** - S3 / local file storage abstraction
- **Rate limiting** - Multi-tier adaptive rate limiting
- **Authorization** - Pundit-style policy-based authz (`Policy` trait)
- **WebSocket** - Real-time push to connected clients
- **Metrics** - Prometheus-compatible `/metrics` endpoint
- **Admin** - CLI/TUI admin commands

### Frontend (Angular / `app/`)

- **Auth service** - Login, registration, JWT token management and refresh
- **HTTP interceptor** - Transparent token injection on every outbound request
- **Realtime service** - WebSocket connection to backend push events
- **Offline sync** - Local IndexedDB via Dexie + delta sync with the backend
- **Storage service** - File upload/download against backend storage
- **Billing service** - Stripe checkout and portal redirects
- **Devtools** - Dev overlay and mail preview for local development

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
erno = { git = "https://github.com/tomekpiotrowski/erno" }
```

## Quick Start

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
        // Add your routes here
        .with_state(state)
}

fn job_registry() -> JobRegistry {
    JobRegistry::new()
        // Register your jobs here
}

fn job_schedule() -> Vec<Box<dyn ScheduledJob>> {
    vec![
        // Add your scheduled jobs here
    ]
}
```

## Features

### Job Processing

Schedule and run background jobs with advisory locks:

```rust
use erno::jobs::*;

#[derive(Clone)]
struct EmailJob;

#[async_trait]
impl ScheduledJob for EmailJob {
    fn schedule(&self) -> &str {
        "0 */5 * * * *" // Every 5 minutes
    }

    async fn execute(&self, ctx: &JobContext) -> Result<JobResult> {
        // Your job logic here
        Ok(JobResult::success())
    }
}

// Register and start
let registry = JobRegistry::new()
    .register(EmailJob);

let scheduler = Scheduler::new(registry, db);
scheduler.start().await?;
```

### Authentication

Protect routes with JWT authentication:

```rust
use erno::auth::prelude::*;

async fn protected_handler(
    CurrentUser(user): CurrentUser,
) -> impl IntoResponse {
    Json(json!({ "user_id": user.id }))
}
```

### Rate Limiting

Apply rate limits to your routes:

```rust
use erno::rate_limiting::*;

let app = Router::new()
    .route("/api/data", get(handler))
    .layer(RateLimitMiddleware::new(
        RateLimitAction::ApiCall,
        60, // requests
        Duration::from_secs(60) // per minute
    ));
```

## Development

### Running Tests

```bash
cargo test --all-features
```

### Formatting

```bash
cargo fmt
```

### Linting

```bash
cargo clippy --all-features -- -D warnings
```

## Documentation

Generate and open documentation locally:

```bash
cargo doc --open
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
