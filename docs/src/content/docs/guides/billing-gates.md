---
title: Gate features with billing
description: Require an active subscription on the API and check plans in the Angular app
---

Erno models Stripe, gift, and trial subscriptions behind one server-side gate. Protect premium handlers with `ActiveSubscription`, then use `ErnoBillingService` on the client for checkout and UI state.

Deep reference: [Billing (API)](/api/billing/), [Billing (App)](/app/billing/).

## 1. Mount billing routes

```rust
use erno::billing::billing_router;

fn router(app: App) -> Router {
    Router::new()
        .nest("/api/billing", billing_router(app.clone()))
        // your routes...
        .with_state(app)
}
```

Configure Stripe keys and plan IDs in TOML / `APP_*` env (see API billing docs).

## 2. Protect a premium endpoint

```rust
use erno::auth::prelude::*;
use erno::billing::ActiveSubscription;

async fn export_report(
    sub: ActiveSubscription,
    CurrentUser { user, .. }: CurrentUser,
) -> impl IntoResponse {
    // sub.plan is available if you branch on plan tiers
    Json(json!({ "user": user.id, "plan": sub.plan }))
}
```

Without an active subscription (Stripe, gift, or trial), the extractor returns **402 Payment Required**. The happy path reads cached columns on the user row — no extra join.

Optional: branch on plan name:

```rust
if sub.plan != "pro" {
    return (StatusCode::FORBIDDEN, "Pro plan required").into_response();
}
```

## 3. Start trials for new users

After registration (handler or job):

```rust
use erno::billing::create_trial;

create_trial(&app.db, user.id, "pro", 14).await?;
```

Idempotent — safe if the user already has a trial.

## 4. Client: show plan and upgrade

```typescript
import { ErnoBillingService } from 'erno-angular';

constructor(private billing: ErnoBillingService) {}

ngOnInit() {
  this.billing.getSubscription().subscribe(sub => {
    this.hasAccess = !!sub;
    this.plan = sub?.plan ?? null;
  });
}

checkout() {
  this.billing.getCheckoutUrl('pro').subscribe(({ url }) => {
    window.location.href = url;
  });
}

portal() {
  this.billing.getPortalUrl().subscribe(({ url }) => {
    window.location.href = url;
  });
}
```

| Goal | Call |
|------|------|
| Gate UI (soft) | `getSubscription()` → hide/show features |
| Start paid plan | `getCheckoutUrl(plan)` → redirect |
| Manage payment method / cancel | `getPortalUrl()` → redirect |

## 5. Handle 402 in the UI

When a user hits a gated API without a subscription, the interceptor does not special-case 402 — handle it in the feature:

```typescript
this.http.get('/api/reports/export').subscribe({
  error: err => {
    if (err.status === 402) {
      this.alerts.warn('Subscription required');
      this.checkout();
    }
  },
});
```

Never rely on client checks alone: always keep `ActiveSubscription` on the server.

## Gift and admin

Operators can grant gift subscriptions via [`erno admin`](/api/console/) without going through Stripe. Those users also pass `ActiveSubscription` while the gift is active.

## Checklist

| Layer | Requirement |
|-------|-------------|
| Config | Stripe (or gifts/trials only) configured |
| Router | `billing_router` nested under your API |
| Handlers | `ActiveSubscription` on paid routes |
| Client | Checkout/portal for conversion; optional soft UI gates |
| Trials | `create_trial` in onboarding if you offer free windows |
