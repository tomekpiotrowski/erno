---
title: Billing
description: ErnoBillingService — subscription status, Stripe checkout, and customer portal
sidebar:
  order: 6
---

`ErnoBillingService` is a thin client for the Erno billing HTTP API: read the current subscription, start Stripe Checkout, or open the customer portal.

> **Backend counterpart**: [Billing (API)](/api/billing/)  
> **Recipe**: [Gate features with billing](/guides/billing-gates/)

## Usage

```typescript
import { ErnoBillingService } from 'erno-angular';

constructor(private billing: ErnoBillingService) {}

loadSubscription() {
  this.billing.getSubscription().subscribe(sub => {
    // null when the user has no active subscription
    // { plan, status, current_period_end } when they do
    this.plan = sub?.plan ?? null;
  });
}

upgrade(plan: string) {
  this.billing.getCheckoutUrl(plan).subscribe(({ url }) => {
    window.location.href = url; // Stripe Checkout
  });
}

manage() {
  this.billing.getPortalUrl().subscribe(({ url }) => {
    window.location.href = url; // Stripe Customer Portal
  });
}
```

## API

| Member | Endpoint | Description |
|--------|----------|-------------|
| `getSubscription()` | `GET /api/billing/subscription` | Active subscription summary, or `null` |
| `getCheckoutUrl(plan)` | `POST /api/billing/checkout` | Stripe Checkout session URL for `plan` |
| `getPortalUrl()` | `POST /api/billing/portal` | Stripe Customer Portal URL |

`ActiveSubscription` shape:

| Field | Type | Description |
|-------|------|-------------|
| `plan` | `string` | Plan identifier (e.g. `"pro"`) |
| `status` | `string` | Provider status string |
| `current_period_end` | `string` | ISO timestamp for the current period end |

## Notes

- Mount `billing_router` on the API and configure Stripe secrets before checkout works. See the [API billing docs](/api/billing/).
- Gift and trial subscriptions appear through the same subscription endpoint when the server considers them active; checkout/portal are Stripe-specific.
- Protect premium **server** routes with `ActiveSubscription` — do not rely on the client alone.
