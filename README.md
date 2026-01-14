# Erno

[![CI](https://github.com/yourusername/erno/workflows/CI/badge.svg)](https://github.com/yourusername/erno/actions)
[![Crates.io](https://img.shields.io/crates/v/erno.svg)](https://crates.io/crates/erno)
[![Documentation](https://docs.rs/erno/badge.svg)](https://docs.rs/erno)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Shared infrastructure library for building REST APIs with Axum.

## Features

- **Job Processing** - Background job scheduler with workers and advisory locks
- **Authentication** - JWT-based authentication with current user extraction
- **Configuration** - Environment-based configuration management
- **Database** - SeaORM integration with migrations
- **Telemetry** - Tracing and observability setup
- **API Utilities** - JSON error handling, validation, rate limiting
- **WebSocket** - WebSocket connection management
- **Console** - Interactive Rhai scripting console

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
