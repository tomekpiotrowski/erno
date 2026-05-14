# erno-angular

Angular library for building Ionic web and mobile apps on top of an [erno](https://github.com/tomekpiotrowski/erno) backend. Provides authentication, offline-first sync, realtime push, file storage, and billing — all wired to the corresponding Rust backend modules.

## Installation

```sh
npm install erno-angular
```

Register the module and provide your backend URL:

```ts
import { ErnoModule } from 'erno-angular';

@NgModule({
  imports: [
    ErnoModule.forRoot({ apiUrl: 'https://api.example.com' }),
  ],
})
export class AppModule {}
```

## Services

| Service | Responsibility |
|---------|---------------|
| `ErnoAuthService` | Login, registration, JWT access + refresh token management |
| `ErnoHttpInterceptor` | Attaches JWT access token to every outbound request; refreshes on 401 |
| `ErnoRealtimeService` | WebSocket connection to backend push events |
| `ErnoDatabaseService` | Local IndexedDB via Dexie for offline-first storage |
| `ErnoSyncService` | Delta sync between local database and backend |
| `ErnoStorageService` | File upload/download against backend storage |
| `ErnoBillingService` | Stripe checkout and customer portal redirects |
| `ErnoDevtoolsComponent` | Dev overlay (mount in dev builds only) |
| `ErnoDevMailService` | Preview outbound emails in development |

## Documentation

Full narrative docs live at `docs/src/content/docs/app/` in the monorepo.
