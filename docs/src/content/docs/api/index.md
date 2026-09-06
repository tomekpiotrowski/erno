---
title: API Overview
description: Overview of the Erno API library modules
sidebar:
  order: 0
---

Erno is a Rust library that provides shared infrastructure for building REST APIs with [Axum](https://github.com/tokio-rs/axum). It bundles the common concerns every SaaS backend needs so you can focus on product logic.

## Modules

### Core

| Module | Description |
|--------|-------------|
| [Manual API setup](./getting-started) | Install the library without the full-stack CLI |
| [Boot & Configuration](./boot) | Application bootstrap, routing, and environment config |
| [Database](./database) | SeaORM integration with migrations |
| [Testing](./testing) | Request specs, factories, and `erno test` |

### Security

| Module | Description |
|--------|-------------|
| [Authentication](./authentication) | JWT-based auth and the `CurrentUser` extractor |
| [Authorization](./authorization) | Policy-based authorization for SeaORM entities |
| [Rate limiting](./rate-limiting) | Per-action request rate limiting middleware |

### Data & realtime

| Module | Description |
|--------|-------------|
| [Sync](./sync) | Offline-first delta synchronization over WebSocket |
| [Sharing](./share) | Secret links and grants, integrated with sync |
| [File storage](./storage) | Local and S3 file storage with polymorphic attachments |
| [WebSocket](./websocket) | WebSocket connection management |

### Product

| Module | Description |
|--------|-------------|
| [Billing](./billing) | Stripe, gift, and trial subscription management |

### Background & ops

| Module | Description |
|--------|-------------|
| [Jobs](./jobs) | Background job scheduling with cron and advisory locks |
| [Email](./email) | Sending HTML and multipart emails via SMTP |
| [Telemetry](./telemetry) | Distributed tracing and Prometheus metrics |
| [Admin console](./console) | HTTP admin API + Angular operator app |
| [Business stats](./business-stats) | Daily SaaS metric snapshots |

## Installation

Add Erno to your `Cargo.toml`:

```toml
[dependencies]
erno = { git = "https://github.com/tomekpiotrowski/erno" }
```

That follows the default branch. `erno new` pins a release tag instead.

New full-stack projects should use [`erno new`](/getting-started/) instead of wiring the crate by hand.
