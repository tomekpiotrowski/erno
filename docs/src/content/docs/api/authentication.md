---
title: Authentication
description: JWT-based authentication and the CurrentUser extractor
sidebar:
  order: 3
---

> **Source**: `api/src/auth/`

Erno ships JWT-based authentication. Access tokens are short-lived (default 15 minutes); refresh tokens last 30 days by default. Both durations are configurable.

## Configuration

```toml
[auth]
secret = "<random 32+ byte string>"
access_token_minutes = 15     # default
refresh_token_days = 30       # default
one_time_token_expiry_hours = 24
```

Generate a suitable secret:

```bash
cargo run -- generate-secret
```

## Protecting routes

Add `CurrentUser` as an extractor to any handler that requires authentication. Erno validates the `Authorization: Bearer <token>` header, looks up the user in the database, and rejects the request with `401 Unauthorized` if anything fails.

```rust
use erno::auth::prelude::*;

async fn get_profile(
    CurrentUser { user, .. }: CurrentUser,
) -> impl IntoResponse {
    Json(json!({ "id": user.id, "email": user.email }))
}
```

## Token versioning

Tokens carry a `ver` claim that is compared against the `token_version` stored on the user record. When a user logs out or changes their password, `token_version` is incremented, which immediately invalidates all previously issued tokens — no token blocklist needed.

## Loading profile data

`CurrentUser` is generic over a `LoadForUser` profile type. Use the plain `CurrentUser` when you only need the base user, or parameterize it to load additional data in the same extractor call:

```rust
// Just the user
async fn handler(CurrentUser { user, .. }: CurrentUser) { ... }

// User + app-specific profile loaded from DB
async fn handler(CurrentUser { user, profile }: CurrentUser<Profile>) { ... }
```

Implement `LoadForUser` on your profile model:

```rust
use erno::auth::prelude::*;

#[async_trait]
impl LoadForUser for Profile {
    async fn load_for_user(
        user_id: Uuid,
        db: &DatabaseConnection,
    ) -> Result<Self, AuthError> {
        profile::Entity::find_by_id(user_id)
            .one(db)
            .await
            .map_err(|_| AuthError::DatabaseError)?
            .ok_or(AuthError::Unauthorized)
    }
}
```

## Built-in auth routes

Mount the built-in auth router to get registration, login, and password reset endpoints:

```rust
use erno::auth::router::auth_router;

fn router(app: App) -> Router {
    Router::new()
        .nest("/auth", auth_router())
        .with_state(app)
}
```

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/auth/register` | Create user account |
| `POST` | `/auth/login` | Issue access + refresh tokens |
| `POST` | `/auth/refresh` | Exchange refresh token for new access token |
| `POST` | `/auth/logout` | Invalidate tokens (increments token_version) |
| `POST` | `/auth/email/verify` | Verify email address via one-time token |
| `POST` | `/auth/email/resend-verification` | Re-send the verification email |
| `POST` | `/auth/password-reset/request` | Send password reset email |
| `POST` | `/auth/password-reset/confirm` | Apply new password via one-time token |
| `DELETE` | `/api/account` | Permanently delete the account and all its data |

## Account deletion

`DELETE /api/account` lets a logged-in user permanently delete their account — a hard
requirement for App Store / Play Store submissions. It is mounted by `auth_router` at the top
level (`/api/account`, not under `/auth`).

The request must include the current password; a mismatch returns `403`:

```http
DELETE /api/account
Authorization: Bearer <access-token>
Content-Type: application/json

{ "password": "current-password" }
```

On success it returns `204 No Content`. In one transaction Erno deletes the `users` row — which
cascades to `user_tokens` and the Stripe/trial/gift subscription tables — and then enqueues
retryable background jobs to **cancel the Stripe subscription** and **delete the user's uploaded
files**.

`ErnoAuthService.deleteAccount(password)` in `erno-angular` wraps the call: on success it clears the
local session and wipes the locally cached sync state in IndexedDB (sync cursors + queued offline
mutations) so nothing remains on a shared device. If your app keeps its own IndexedDB stores for
synced entities, clear those too in your deletion success handler.

### Deleting app-owned data

Erno only owns the tables above. For your own per-user tables you have two options (use either or
both):

1. **`ON DELETE CASCADE`** — give your tables a foreign key to `users` with cascade delete, and they
   are removed automatically when the user row goes.
2. **`UserDataDeleter` hook** — implement the trait and register it; it runs inside the deletion
   transaction (before the user row is removed), so returning `Err` aborts and rolls back the whole
   deletion.

```rust
use erno::account::UserDataDeleter;
use sea_orm::{DatabaseTransaction, DbErr};
use uuid::Uuid;

struct MyDataDeleter;

#[async_trait::async_trait]
impl UserDataDeleter for MyDataDeleter {
    async fn delete_user_data(&self, txn: &DatabaseTransaction, user_id: Uuid) -> Result<(), DbErr> {
        my_table::Entity::delete_many()
            .filter(my_table::Column::UserId.eq(user_id))
            .exec(txn)
            .await?;
        Ok(())
    }
}

// wire it during boot
BootConfig::new(app_info, app_router, registry, schedule)
    .on_delete_user(std::sync::Arc::new(MyDataDeleter));
```

The `cancel_stripe_subscription` and `delete_user_files` jobs are registered automatically — make
sure a worker pool lists them in `jobs` (the `erno new` scaffold already does). If either job
exhausts its retries it logs an error via its `on_permanent_failure` hook (see the
[jobs docs](/api/jobs/#retries-and-failure-handling)); register an app-wide `JobFailureHandler` to be
alerted.

Admin operators can run the same purge from the console TUI (select a user → `x` → confirm).
