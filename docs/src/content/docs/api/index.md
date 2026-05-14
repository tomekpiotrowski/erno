---
title: API Overview
description: Overview of the Erno API library modules
sidebar:
  order: 0
---

Erno is a Rust library that provides shared infrastructure for building REST APIs with [Axum](https://github.com/tokio-rs/axum). It bundles the common concerns every SaaS backend needs so you can focus on product logic.

## Modules

| Module | Description |
|--------|-------------|
| [Getting Started](./getting-started) | Installation and minimal working example |
| [Boot & Configuration](./boot) | Application bootstrap, routing, and environment config |
| [Authentication](./authentication) | JWT-based auth and the `CurrentUser` extractor |
| [Database](./database) | SeaORM integration with migrations |
| [Jobs](./jobs) | Background job scheduling with cron and advisory locks |
| [Rate Limiting](./rate-limiting) | Per-action request rate limiting middleware |
| [WebSocket](./websocket) | WebSocket connection management |
| [Telemetry](./telemetry) | Distributed tracing and Prometheus metrics |
| [Admin TUI](./console) | Interactive terminal UI for user and job administration |
| [Billing](./billing) | Stripe, gift, and trial subscription management |
| [File Storage](./storage) | Local and S3 file storage with polymorphic attachments |
| [Authorization](./authorization) | Policy-based authorization for SeaORM entities |
| [Email](./email) | Sending HTML and multipart emails via SMTP |
| [Sync](./sync) | Offline-first delta synchronization over WebSocket |

## Installation

Add Erno to your `Cargo.toml`:

```toml
[dependencies]
erno = { git = "https://github.com/tomekpiotrowski/erno" }
```
