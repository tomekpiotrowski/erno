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
cargo run -- db migrate up

# Roll back one migration
cargo run -- db migrate down

# Show applied and pending migrations
cargo run -- db migrate status

# Roll back all migrations, then apply them again
cargo run -- db migrate reset

# Reapply the last migration (down, then up)
cargo run -- db migrate reapply

# Open a psql session
cargo run -- db console

# Drop all tables and types, then migrate up
cargo run -- db reset
```

`db reset` wipes objects inside the existing database and reapplies every migration. It does not `DROP DATABASE`, so it works when the app role is not the database owner. `db migrate reset` instead rolls migrations back through their `down` methods, which fails if a migration is irreversible.

## Test utilities

Request-spec helpers (`setup_test`, factories, `TestUtils`) live behind the `test-utils` feature. They boot from your [`BootConfig`](/api/boot/) and wrap each example in a rolled-back transaction. See **[Testing](/api/testing/)** for the full guide.

```toml
[dev-dependencies]
erno = { git = "https://github.com/tomekpiotrowski/erno", features = ["test-utils"] }
```

```rust
use erno::tests::{no_fixtures, setup_test};

#[tokio::test]
async fn health_is_public() {
    let t = setup_test::<Migrator, _>(boot_config(), no_fixtures).await;
    assert_eq!(t.server.get("/api/health").await.status_code(), 200);
}
```
