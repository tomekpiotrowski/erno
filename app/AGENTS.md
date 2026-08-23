# Erno App

Angular 22 library (`erno-angular`) that Ionic apps consume for web and mobile. All commands below run from this directory (`app/`).

## Building & testing

```sh
ng build erno-angular    # build the library into dist/
ng test                  # Vitest unit tests
ng serve                 # dev server on :4200 (demo app)
```

## Key services

| Service | Path | Responsibility |
|---------|------|---------------|
| `ErnoAuthService` | `auth/erno-auth.service` | Login, registration, JWT access + refresh token management |
| `ErnoHttpInterceptor` | `http/erno-http.interceptor` | Attaches JWT access token to every outbound HTTP request; handles 401 refresh |
| `ErnoRealtimeService` | `realtime/erno-realtime.service` | WebSocket connection to backend push events; suspends on background, reconnects on foreground |
| `ErnoAppStateService` | `app-state/erno-app-state.service` | Foreground/background state via `@capacitor/app` (optional) with `visibilitychange` web fallback |
| `ErnoDatabaseService` | `sync/erno-database.service` | Local IndexedDB via Dexie for offline-first storage |
| `ErnoSyncService` | `sync/erno-sync.service` | Delta sync between local IndexedDB and backend |
| `ErnoStorageService` | `storage/erno-storage.service` | File upload/download against backend S3/local storage |
| `ErnoBillingService` | `billing/erno-billing.service` | Stripe checkout and customer portal redirects |
| `ErnoShareService` | `share/erno-share.service` | Create/list/revoke shares and grants; fragment-based share URLs |
| `ErnoSharedViewService` | `share/erno-shared-view.service` | Online-only in-memory view of shared data (delta + live push) |
| `ErnoErrorReporterService` | `errors/erno-error-reporter.service` | Reports uncaught errors to a monitoring collector; installs a global `ErrorHandler` |
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
| `ErnoAuthService` / `ErnoHttpInterceptor` | `authentication.md` |
| `ErnoSyncService` / `ErnoDatabaseService` | `sync.md` |
| `ErnoRealtimeService` / `ErnoAppStateService` | `realtime.md` |
| `ErnoStorageService` | `storage.md` |
| `ErnoBillingService` | `billing.md` |
| `ErnoShareService` / `ErnoSharedViewService` | `share.md` |
| `ErnoErrorReporterService` / `ErnoErrorHandler` | `error-reporting.md` |
| `ErnoDevtoolsComponent` / `ErnoDevMailService` / `ErnoAlertsService` | `devtools.md` |

Cross-cutting guides: `docs/src/content/docs/guides/` (e.g. sync end-to-end, billing gates).

**If you change a service's public API, configuration, or observable behaviour, update the corresponding doc page.**
