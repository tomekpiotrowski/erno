---
title: App Overview
description: erno-angular — Angular client library for Erno backends
sidebar:
  order: 0
---

`erno-angular` is an Angular 20 library that wires an Angular or Ionic app to an Erno backend. It provides auth, offline-first sync, file storage, billing, realtime push, and developer tooling as injectable Angular services.

## Installation

The `erno new` CLI command sets this up automatically. For manual installation:

```sh
npm install erno-angular
```

During local development against an unpublished build, install a packed tarball instead of linking the dist folder directly:

```sh
# From the erno repo root
cd app && ng build erno-angular
(cd dist && npm pack --silent ./erno-angular)

# In your app
npm install file:/path/to/erno/app/dist/erno-angular-0.0.1.tgz
# or use erno new --erno-path <erno-dir> to generate this reference automatically
```

## Setup

Import `ErnoModule` in your `app.config.ts` (standalone) or `AppModule` (NgModule):

```typescript
// standalone (Angular 17+)
import { ApplicationConfig, importProvidersFrom } from '@angular/core';
import { ErnoModule } from 'erno-angular';

export const appConfig: ApplicationConfig = {
  providers: [
    importProvidersFrom(
      ErnoModule.forRoot({
        baseUrl: 'http://localhost:3000',
        wsUrl: 'ws://localhost:3000',
      })
    ),
  ],
};
```

`ErnoModule.forRoot()` registers all services and wires up the HTTP interceptor that attaches JWT tokens to every outbound request.

## Services

| Service | Import | Responsibility |
|---------|--------|---------------|
| `ErnoAuthService` | `erno-angular` | Login, registration, JWT access + refresh token management |
| `ErnoHttpInterceptor` | auto-registered | Attaches JWT to requests; handles 401 silent refresh |
| `ErnoRealtimeService` | `erno-angular` | WebSocket connection to backend push events |
| `ErnoDatabaseService` | `erno-angular` | Local IndexedDB via Dexie for offline storage |
| `ErnoSyncService` | `erno-angular` | Delta sync between local Dexie store and backend |
| `ErnoStorageService` | `erno-angular` | File upload/download against backend S3/local storage |
| `ErnoBillingService` | `erno-angular` | Stripe checkout and customer portal redirects |
| `ErnoDevtoolsComponent` | `erno-angular` | Dev overlay (add `<erno-devtools>` in dev builds) |
| `ErnoDevMailService` | `erno-angular` | Preview outbound emails without SMTP in development |

## Configuration

| Key | Description |
|-----|-------------|
| `baseUrl` | Base URL of the Erno API (e.g. `http://localhost:3000`) |
| `wsUrl` | WebSocket URL of the Erno API (e.g. `ws://localhost:3000`) |

## Architecture notes

- **Mirrors backend modules** — each service corresponds to a module in `api/src/`
- **Token flow** — `ErnoAuthService` stores access + refresh tokens in `localStorage`; `ErnoHttpInterceptor` attaches them automatically and triggers a silent refresh on 401
- **Offline-first** — `ErnoDatabaseService` wraps Dexie (IndexedDB); `ErnoSyncService` pulls deltas from the backend sync endpoints and writes them to the local store
- **Target consumers** — Angular 20+ apps including Ionic/Capacitor; no Ionic-specific code in the library itself
