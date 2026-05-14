# Erno App

Angular 20 library (`erno-angular`) that Ionic apps consume for web and mobile. All commands below run from this directory (`app/`).

## Building & testing

```sh
ng build erno-angular    # build the library into dist/
ng test                  # Karma unit tests
ng serve                 # dev server on :4200 (demo app)
```

## Key services

| Service | Path | Responsibility |
|---------|------|---------------|
| `ErnoAuthService` | `auth/erno-auth.service` | Login, registration, JWT access + refresh token management |
| `ErnoHttpInterceptor` | `http/erno-http.interceptor` | Attaches JWT access token to every outbound HTTP request; handles 401 refresh |
| `ErnoRealtimeService` | `realtime/erno-realtime.service` | WebSocket connection to backend push events |
| `ErnoDatabaseService` | `sync/erno-database.service` | Local IndexedDB via Dexie for offline-first storage |
| `ErnoSyncService` | `sync/erno-sync.service` | Delta sync between local IndexedDB and backend |
| `ErnoStorageService` | `storage/erno-storage.service` | File upload/download against backend S3/local storage |
| `ErnoBillingService` | `billing/erno-billing.service` | Stripe checkout and customer portal redirects |
| `ErnoDevtoolsComponent` | `devtools/erno-devtools.component` | Dev overlay for local development |
| `ErnoDevMailService` | `devtools/erno-dev-mail.service` | Preview outbound emails in dev without SMTP |

## Architecture notes

- **Library package**: consuming apps install `erno-angular` as an npm dependency and import `ErnoModule`
- **Target consumers**: Ionic apps (Angular-compatible); no Ionic-specific code in this library
- **Offline-first**: `ErnoDatabaseService` wraps Dexie (IndexedDB); `ErnoSyncService` pushes/pulls deltas against the backend sync endpoints
- **Token flow**: `ErnoAuthService` stores access + refresh tokens; `ErnoHttpInterceptor` attaches them automatically and triggers refresh on 401
- **Mirrors backend modules**: each service corresponds to a backend module in `api/src/`

## Documentation

Narrative docs for each service live in `docs/src/content/docs/app/`:

| Service | Doc page |
|---------|---------|
| `ErnoAuthService` | `authentication.md` |
| `ErnoSyncService` / `ErnoDatabaseService` | `sync.md` |
| `ErnoRealtimeService` | `realtime.md` |
| `ErnoStorageService` | `storage.md` |
| `ErnoBillingService` | `billing.md` |

**If you change a service's public API, configuration, or observable behaviour, update the corresponding doc page.**
