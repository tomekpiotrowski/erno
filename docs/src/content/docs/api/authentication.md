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
