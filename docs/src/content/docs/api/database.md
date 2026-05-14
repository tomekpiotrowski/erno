---
title: Database
description: SeaORM integration, migrations, and the database connection
sidebar:
  order: 4
---

> **Source**: `api/src/database.rs`

Erno uses [SeaORM](https://www.sea-ql.org/SeaORM/) for database access on top of PostgreSQL. The connection pool is managed automatically; you access it via `app.db`.

## Configuration

```toml
[database]
url = "postgres://user:password@localhost/mydb"
```

## Running queries

The `DatabaseConnection` is available on every `App` instance:

```rust
use sea_orm::EntityTrait;

async fn list_users(State(app): State<App>) -> impl IntoResponse {
    let users = user::Entity::find()
        .all(&app.db)
        .await
        .unwrap();
    Json(users)
}
```

## Migrations

Erno runs migrations on startup via the `MigratorTrait` type parameter passed to `boot`. Define your migration crate the standard SeaORM way and pass your `Migrator` type:

```rust
#[tokio::main]
async fn main() {
    boot::<migration::Migrator>(boot_config()).await;
}
```

Erno's own schema (users, jobs, etc.) ships as an embedded `ErnoCombinedMigration` that your migrator should include:

```rust
use erno::database::migrations::ErnoCombinedMigration;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(ErnoCombinedMigration),
        ];
        // Add your own migrations here
        migrations
    }
}
```

## CLI commands

```bash
# Apply pending migrations
cargo run -- migrate

# Reset database (drops all tables, re-runs migrations)
cargo run -- db-reset

# Show registered routes
cargo run -- routes
```

## Test utilities

The `test-utils` feature exposes helpers for integration tests that spin up an isolated database transaction per test:

```toml
[dev-dependencies]
erno = { git = "...", features = ["test-utils"] }
```

```rust
#[tokio::test]
async fn test_create_user() {
    let (app, _guard) = erno::tests::setup_test().await;
    // Each test runs inside a transaction that is rolled back on drop
}
```
