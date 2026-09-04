---
title: Testing
description: Request specs, factories, and erno test — the consumer test harness
sidebar:
  order: 3
---

Erno apps test at two HTTP-adjacent levels:

| Level | Where | Style |
|-------|--------|--------|
| **Unit** | `#[cfg(test)]` next to the code | `lets_expect!` for domain math |
| **Request** | `api/tests/<family>.rs` | `#[tokio::test]` + `assert_eq!(status)` |

Do **not** retest login, register, password reset, or admin — those live in this crate. Test your routes, `401` on your `CurrentUser` handlers, and a **unit** test of your `UserDataDeleter` if you have one.

Completeness means every input the code accepts has an example for empty, missing, malformed, boundary, and unexpected values — and for the error they produce. There is no coverage-percentage gate.

The Angular app has its own three types (unit, feature, e2e). See [Getting started](/getting-started/) and `erno test`.

## Boot the app you ship

`setup_test` takes the same [`BootConfig`](/api/boot/) `boot()` uses. Sync registry, `UserDataDeleter`, ExtraConfig, and the job-failure hook come along. Do not rebuild `App` by hand.

```toml
[dev-dependencies]
erno = { git = "https://github.com/tomekpiotrowski/erno", tag = "v0.2.1", features = ["test-utils"] }
```

```rust
use erno::tests::{no_fixtures, setup_test};

#[tokio::test]
async fn health_is_public() {
    let t = setup_test::<Migrator, _>(boot_config(), no_fixtures).await;
    let response = t.server.get("/api/health").await;
    assert_eq!(response.status_code(), 200);
}
```

Each call creates a one-connection pool and `BEGIN`. Dropping `TestUtils` issues `ROLLBACK`. Schema drop + migrate + optional fixtures run once per process.

`erno::tests::test_boot(router)` builds a minimal `BootConfig<()>` for this crate's own tests.

## Factories, not YAML fixtures

Request specs need per-example rows. Use factories:

```rust
use erno::tests::{bearer, no_fixtures, setup_test, verified_user};

#[tokio::test]
async fn list_is_owner_only() {
    let t = setup_test::<Migrator, _>(boot_config(), no_fixtures).await;
    let me = verified_user(&t.db, "a@example.com", "password123").await;
    let response = t
        .server
        .get("/api/sessions")
        .add_header("Authorization", bearer(&t, &me))
        .await;
    assert_eq!(response.status_code(), 200);
}
```

Also exported: `unverified_user`, `no_fixtures`.

`FixtureLoader` runs **once** on the schema-init connection (committed). Use it only for stable reference data every example should see. Mutations in a test still roll back. There is no YAML/JSON fixture format.

## What `TestUtils` gives you

| Field / method | Use |
|----------------|-----|
| `server` | `axum_test::TestServer` |
| `db` | Same connection as the app (inside the test transaction) |
| `sent_emails()` | Mock mailer records |
| `enqueued_jobs()` / `enqueued_jobs_of_type` | Mock job queue |
| `execute_job::<J>(args)` | Run a job against the test DB |

## `erno test`

From the project root (or any subdirectory):

```sh
erno test                    # every default package, e2e last
erno test --api
erno test --app              # Karma unit + feature
erno test --e2e              # Playwright against a live test API
erno test --no-e2e
erno test --package puzzles  # one package by name
erno test --api -- health    # pass-through; one package only
```

The test database in `api/config/test.toml` is created if missing. Which packages exist, and what each one runs, comes from [`erno.toml`](/cli/#the-package-manifest) in the project root.

`erno test --e2e` binds the API and `ng serve` to unused ports (not `3000`/`4200`) and passes `API_URL` / `APP_URL` into Playwright and the app bundle. It compiles the API (`cargo build`) before waiting on `/liveness`, so a cold compile does not burn the boot window. A leftover `erno dev` on the usual ports cannot satisfy `/liveness`.

Apps from `erno new` ship `.github/workflows/ci.yml` that mirrors `erno lint` and `erno test`.

## Account deletion

The hook contract and `DELETE /api/account` HTTP behaviour are tested in Erno. Your app unit-tests the deleter impl (insert rows, call `delete_user_data`, assert they are gone). Wiring is covered automatically when request tests pass `boot_config()`.
