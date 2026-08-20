---
title: App Overview
description: erno-angular — Angular client library for Erno backends
sidebar:
  order: 0
---

`erno-angular` is an Angular 22 library that wires an Angular or Ionic app to an Erno backend. It provides auth, offline-first sync, file storage, billing, sharing, realtime push, and developer tooling as injectable Angular services.

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

Call `provideErno` from a standalone `bootstrapApplication` (what `erno new` writes). NgModule apps keep `ErnoModule.forRoot()`, which delegates to the same providers.

```typescript
import { provideErno } from 'erno-angular';

bootstrapApplication(AppComponent, {
  providers: [
    provideErno({
      baseUrl: 'http://localhost:3000',
      wsUrl: 'ws://localhost:3000',
    }),
  ],
});
```

```typescript
// NgModule apps
ErnoModule.forRoot({
  baseUrl: 'http://localhost:3000',
  wsUrl: 'ws://localhost:3000',
})
```

Either form registers all services and the HTTP interceptor that attaches JWT tokens to every outbound request.

## Services

| Service | Doc | Responsibility |
|---------|-----|----------------|
| `ErnoAuthService` | [Authentication](./authentication) | Login, registration, JWT access + refresh token management |
| `ErnoHttpInterceptor` | [Authentication](./authentication#http-interceptor) | Attaches JWT to requests; handles 401 silent refresh |
| `ErnoRealtimeService` | [Realtime](./realtime) | WebSocket connection to backend push events |
| `ErnoAppStateService` | [Realtime](./realtime#app-state) | Tracks foreground/background state (Capacitor + web fallback) |
| `ErnoDatabaseService` | [Sync](./sync) | Local IndexedDB via Dexie for offline storage |
| `ErnoSyncService` | [Sync](./sync) | Delta sync between local Dexie store and backend |
| `ErnoStorageService` | [File storage](./storage) | File upload/download against backend S3/local storage |
| `ErnoBillingService` | [Billing](./billing) | Stripe checkout and customer portal redirects |
| `ErnoShareService` | [Sharing](./share) | Create/list/revoke shares and grants |
| `ErnoSharedViewService` | [Sharing](./share#consumer-online-shared-view) | Online-only in-memory view of shared data |
| `ErnoDevtoolsComponent` | [Devtools](./devtools) | Dev overlay (`<erno-devtools>`) |
| `ErnoDevMailService` | [Devtools](./devtools) | Preview outbound emails without SMTP in development |
| `ErnoAlertsService` | [Devtools](./devtools#alerts-ernoalertsservice) | Queued Ionic toast notifications |

## Configuration

| Key | Description |
|-----|-------------|
| `baseUrl` | Base URL of the Erno API (e.g. `http://localhost:3000`) |
| `wsUrl` | WebSocket URL of the Erno API (e.g. `ws://localhost:3000`) |

## Architecture notes

- **Mirrors backend modules** — each service corresponds to a module in `api/src/`
- **Token flow** — `ErnoAuthService` stores access + refresh tokens; `ErnoHttpInterceptor` attaches them automatically and triggers a silent refresh on 401
- **Offline-first** — `ErnoDatabaseService` wraps Dexie (IndexedDB); `ErnoSyncService` pulls deltas from the backend sync endpoints and writes them to the local store
- **Target consumers** — Angular 22 apps including Ionic 9 / Capacitor; no Ionic-specific code in the library itself (except optional toast alerts)

## See also

- [Getting started](/getting-started/) — scaffold with `erno new`
- [Architecture](/architecture/) — how API and app talk
- [Sync an entity end-to-end](/guides/sync-an-entity/)
