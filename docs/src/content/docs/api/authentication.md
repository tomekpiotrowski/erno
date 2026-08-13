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
cargo run -- generate-jwt-secret
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

> **Client counterpart**: [Authentication (App)](/app/authentication/)

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
| `DELETE` | `/account` | Permanently delete the account and all its data |

## Account deletion

`DELETE /api/account` lets a logged-in user permanently delete their account. In-app account
deletion is a hard requirement for App Store submissions (guideline 5.1.1(v)): it must be
initiated inside the app, must really delete the account rather than deactivate it, and must not
require contacting support. This endpoint plus the `erno-angular` client and the scaffold UI
cover those requirements. It is mounted by `auth_router` at the top level (`/api/account`, not
under `/auth`).

> **Play Store note:** Google Play additionally requires a *web-accessible* account-deletion
> request path for users who have uninstalled the app. An authenticated API endpoint alone does
> not satisfy that — if you ship on Android you must also host a web page where users can
> request deletion, and disclose it in the Play Console Data safety form.

The request must include the current password in the `X-Confirm-Password` header (a header, not
a JSON body, because some proxies and CDNs strip bodies on `DELETE`). A mismatch returns `403`;
a missing header returns `400`:

```http
DELETE /api/account
Authorization: Bearer <access-token>
X-Confirm-Password: current-password
```

On success it returns `204 No Content`. In one transaction Erno deletes the `users` row — which
cascades to `user_tokens` and the Stripe/trial/gift subscription tables — and then enqueues
retryable background jobs to:

- **cancel every active/past-due Stripe subscription** (a user can have several, one per
  checkout — all are cancelled),
- **delete the Stripe customer object**, removing the email, name, and payment methods held on
  Stripe's side (Stripe retains invoices for its own tax obligations regardless),
- **delete the user's uploaded files** (`record_type = "user"` attachments and their blobs).

The account itself is deleted immediately; the cleanup jobs run in the background with retries.
If any of them exhausts its retries it logs an error via its `on_permanent_failure` hook (see
the [jobs docs](/api/jobs/#retries-and-failure-handling)) — register an app-wide
`JobFailureHandler` to be alerted, since a permanently-failed cancel job means the ex-user may
still be billed.

`ErnoAuthService.deleteAccount(password)` in `erno-angular` wraps the call: on success it clears
the local session and wipes the locally cached sync state in IndexedDB (sync cursors + queued
offline mutations) so nothing remains on a shared device. If your app keeps its own IndexedDB
stores for synced entities, clear those too in your deletion success handler.

### Deleting app-owned data

Erno only owns the tables above. For your own per-user tables you have two options (use either
or both):

1. **`ON DELETE CASCADE`** — give your tables a foreign key to `users` with cascade delete, and
   they are removed automatically when the user row goes. **This does not remove files**: a
   cascade deletes your rows but leaves their `file_attachments` rows, `files` rows, and stored
   blobs behind (attachments have no foreign key to your tables). For files, use the hook below.
2. **`UserDataDeleter` hook** — implement the trait and register it; it runs inside the deletion
   transaction (before the user row is removed), so returning `Err` aborts and rolls back the
   whole deletion. The hook also receives the `JobQueue`, so you can enqueue
   `delete_record_attachments` jobs — one per app-owned record — atomically with the deletion.

```rust
use erno::account::UserDataDeleter;
use erno::job_queue::JobQueue;
use erno::storage::delete_record_attachments_job;
use sea_orm::{DatabaseTransaction, DbErr};
use uuid::Uuid;

struct MyDataDeleter;

#[async_trait::async_trait]
impl UserDataDeleter for MyDataDeleter {
    async fn delete_user_data(
        &self,
        txn: &DatabaseTransaction,
        job_queue: &JobQueue,
        user_id: Uuid,
    ) -> Result<(), DbErr> {
        let posts = post::Entity::find()
            .filter(post::Column::UserId.eq(user_id))
            .all(txn)
            .await?;

        // Remove the files attached to each post (async, retried).
        for post in &posts {
            job_queue
                .enqueue_by_name(
                    txn,
                    delete_record_attachments_job::JOB_NAME,
                    serde_json::json!({ "record_type": "post", "record_id": post.id }),
                )
                .await?;
        }

        // Remove the rows themselves.
        post::Entity::delete_many()
            .filter(post::Column::UserId.eq(user_id))
            .exec(txn)
            .await?;
        Ok(())
    }
}

// wire it during boot
BootConfig::new(app_info, app_router, registry, schedule)
    .on_delete_user(std::sync::Arc::new(MyDataDeleter));
```

The `cancel_stripe_subscription`, `delete_stripe_customer`, `delete_user_files`, and
`delete_record_attachments` jobs are registered automatically — make sure a worker pool lists
them in `jobs` (the `erno new` scaffold already does). If a registered job type has no worker
pool, Erno panics at boot naming the uncovered types (see
[jobs docs](/api/jobs/#worker-coverage-check)) — existing apps upgrading Erno must add the new
job types to a pool.

Admin operators can run the same purge from the [admin app](../console) user detail page, which calls `DELETE /admin/api/users/{id}` on the server.

### App Store review notes

- **Subscriptions:** deleting the account cancels Stripe-billed subscriptions (above). If your
  app *also* sells Apple in-app subscriptions, deletion does **not** cancel those — Apple
  requires you to tell users this in the deletion flow and link to Manage Subscriptions.
- **Sign in with Apple:** Erno's auth is email/password only, so there is no Apple token to
  revoke today. If you add Sign in with Apple, Apple requires revoking the user's token
  (`appleid.apple.com/auth/revoke`) on deletion — that is a network call and must **not** run
  inside the deletion transaction; enqueue it as a job from `UserDataDeleter` instead.
