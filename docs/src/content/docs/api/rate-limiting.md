---
title: Rate Limiting
description: Per-action multi-tier rate limiting middleware
sidebar:
  order: 6
---

> **Source**: `api/src/rate_limiting/`

Erno's rate limiter is keyed by **client IP + action name**. It uses a multi-tier model so you can catch both fast bursts and sustained abuse with a single configuration.

## Configuration

```toml
[rate_limiting]
enabled = true
trust_proxy = false          # set true when behind nginx/Caddy
default_window_secs = 60
default_max_requests = 100
backoff_multiplier = 2.0

# Per-action overrides — multiple tiers, all must pass
[rate_limiting.actions.user_create]
tiers = [
  { window_secs = 5,    max_requests = 2  },   # burst
  { window_secs = 60,   max_requests = 5  },   # per minute
  { window_secs = 3600, max_requests = 20 },   # per hour
]
```

All tiers are evaluated; a request is blocked if **any** tier is exceeded.

## Built-in action limits

Erno pre-configures conservative limits for sensitive auth endpoints:

| Action | Burst (5s) | Per minute | Per hour |
|--------|-----------|------------|---------|
| `user_create` | 2 | 5 | 20 |
| `user_login` | 5 | 10 | 30 |
| `password_reset_request` | 2 | 5 | 10 |
| `password_reset_confirm` | 5 | 10 | 20 |
| `resend_verification` | 2 | 5 | 10 |

Any action not explicitly configured falls back to the global `default_window_secs` / `default_max_requests`.

## Tagging routes with an action

Use `RateLimitActionExt` to attach an action name to a request. The middleware reads it from request extensions:

```rust
use erno::rate_limiting::{RateLimitActionExt, rate_limit_middleware};
use axum::middleware;

fn router(app: App) -> Router {
    Router::new()
        .route("/api/expensive", post(expensive_handler))
        .layer(middleware::from_fn_with_state(
            app.rate_limit_state.clone(),
            rate_limit_middleware,
        ))
        // Tag this route's action name:
        .layer(middleware::map_request(|mut req: Request<Body>| async {
            req.extensions_mut().insert(RateLimitActionExt("api_expensive".into()));
            req
        }))
        .with_state(app)
}
```

## Proxy configuration

Set `trust_proxy = true` only when running behind a trusted reverse proxy (nginx, Caddy, etc.). Without it, all users behind the same proxy share one rate limit quota because the server sees the proxy's IP, not the real client IP. With it enabled, Erno reads `X-Forwarded-For` and `X-Real-IP` headers.

## Response format

When a rate limit is exceeded, Erno returns:

```
HTTP 429 Too Many Requests
Retry-After: <seconds>

Rate limit exceeded. Please try again later.
```

## Backend

The default backend is in-memory and suitable for single-instance deployments. For multi-replica deployments implement the `RateLimitBackend` trait backed by Redis or another shared store, and supply it via `RateLimitState::with_backend`.
