---
title: Billing
description: Stripe, gift, and trial subscription management
sidebar:
  order: 10
---

> **Source**: `api/src/billing/`

Erno ships three subscription types — Stripe (recurring payments), Gift (admin-assigned), and Trial (time-limited free access). All three are modelled through a shared extractor so route handlers don't need to branch on subscription source.

## Protecting routes

Add `ActiveSubscription` as a handler argument to require an active subscription. It reads cached columns on the user row — no extra database query in the happy path — and returns `402 Payment Required` if the user has no active subscription.

```rust
use erno::billing::ActiveSubscription;
use erno::auth::prelude::*;

async fn premium_endpoint(
    _sub: ActiveSubscription,
    CurrentUser { user, .. }: CurrentUser,
) -> impl IntoResponse {
    Json(json!({ "plan": _sub.plan }))
}
```

`ActiveSubscription` fields:

| Field | Type | Description |
|-------|------|-------------|
| `plan` | `String` | Plan identifier (e.g. `"pro"`) |
| `subscription_type` | `String` | `"stripe"`, `"gift"`, or `"trial"` |

## Inspecting the full subscription record

When you need the full subscription record (expiry date, Stripe IDs, etc.), call `load_current_subscription`. It uses `user.subscription_id` + `user.subscription_type` to do a single PK lookup on the correct table.

```rust
use erno::billing::{load_current_subscription, CurrentSubscription};

let sub = load_current_subscription(&app.db, &user).await;

match sub {
    Some(CurrentSubscription::Stripe(s)) => { /* s.stripe_subscription_id, etc. */ }
    Some(CurrentSubscription::Gift(g))   => { /* g.active_until, etc. */ }
    Some(CurrentSubscription::Trial(t))  => { /* t.active_until, etc. */ }
    None => { /* no active subscription */ }
}
```

## Trial subscriptions

Create a trial for a new user after registration. The call is idempotent — it silently no-ops if the user already has a trial.

```rust
use erno::billing::create_trial;

create_trial(&app.db, user.id, "pro", 14).await?;
```

A common pattern is to call this inside the post-registration flow or in a background job triggered on user creation.

## Billing routes

Mount `billing_router` in your app router to expose the Stripe endpoints:

```rust
use erno::billing::billing_router;

fn router(app: App) -> Router {
    Router::new()
        .nest("/billing", billing_router(app.clone()))
        // ...
        .with_state(app)
}
```

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/billing/checkout` | JWT | Create a Stripe Checkout Session; returns `{ url }` |
| `POST` | `/billing/portal` | JWT | Create a Stripe Customer Portal session; returns `{ url }` |
| `POST` | `/billing/webhooks` | HMAC | Receive Stripe webhook events |
| `POST` | `/billing/admin/gift` | Bearer token | Gift a subscription to a user |

The webhook endpoint validates the `Stripe-Signature` header using `stripe.webhook_secret`. The admin gift endpoint requires the `stripe.admin_token` bearer token.

## Admin gifting

Subscriptions can also be gifted from the [Admin TUI](../console) — no API call needed.

## Configuration

```toml
[stripe]
secret_key = "sk_live_..."
webhook_secret = "whsec_..."
admin_token = "your-admin-bearer-token"
success_url = "https://example.com/success"
cancel_url = "https://example.com/cancel"
portal_return_url = "https://example.com/account"

[stripe.price_ids]
pro = "price_abc123"
enterprise = "price_xyz789"
```

`price_ids` maps your plan names (the strings you pass to `create_trial` and display to users) to Stripe Price IDs. The plan name is stored on the subscription and returned by `ActiveSubscription.plan`.
