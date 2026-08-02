---
title: Authentication
description: ErnoAuthService — login, registration, JWT tokens, and silent refresh
sidebar:
  order: 1
---

`ErnoAuthService` talks to the Erno auth endpoints and holds the current session. Access tokens live in `sessionStorage`; refresh tokens live in `localStorage`. The auto-registered `ErnoHttpInterceptor` attaches the access token to every request against `baseUrl` and refreshes on `401`.

> **Backend counterpart**: [Authentication (API)](/api/authentication/)

## Usage

```typescript
import { ErnoAuthService } from 'erno-angular';

constructor(private auth: ErnoAuthService) {}

login(email: string, password: string) {
  this.auth.login(email, password).subscribe({
    next: () => { /* navigate home */ },
    error: err => console.error(err),
  });
}

ngOnInit() {
  this.auth.currentUser$.subscribe(user => {
    console.log('user', user); // { id, email } | null
  });
}
```

## API

| Member | Description |
|--------|-------------|
| `login(email, password)` | `POST /api/auth/login` — stores tokens and sets `currentUser` |
| `register(email, password)` | `POST /api/auth/register` — does not log the user in until email verification (typical flow) |
| `logout()` | `POST /api/auth/logout` with refresh token, then clears local session |
| `refresh()` | `POST /api/auth/refresh` — rotates tokens |
| `verifyEmail(token)` | `POST /api/auth/email/verify` — stores session on success |
| `requestPasswordReset(email)` | `POST /api/auth/password-reset/request` |
| `confirmPasswordReset(token, password)` | `POST /api/auth/password-reset/confirm` — stores session on success |
| `deleteAccount(password)` | `DELETE /api/account` with `X-Confirm-Password` header; clears session and wipes local IndexedDB |
| `currentUser` / `currentUser$` | Current `{ id, email }` or `null` |
| `accessToken` / `refreshToken` | Raw tokens from storage |

## Token storage

| Token | Storage | Lifetime (server default) |
|-------|---------|---------------------------|
| Access | `sessionStorage` key `erno_access_token` | 15 minutes |
| Refresh | `localStorage` key `erno_refresh_token` | 30 days |

Access tokens are intentionally session-scoped so closing the browser tab drops them; refresh tokens survive reloads so silent re-auth works.

## HTTP interceptor

`ErnoModule.forRoot()` registers `ErnoHttpInterceptor`. For any request whose URL starts with `baseUrl`:

1. Adds `Authorization: Bearer <access_token>` when an access token is present.
2. On `401` (except the refresh endpoint itself), calls `refresh()` once (coalescing concurrent failures), then retries the request.
3. If there is no refresh token, triggers `logout()`.

You normally do not call the interceptor directly.

## Account deletion

```typescript
this.auth.deleteAccount(currentPassword).subscribe({
  next: () => { /* navigate to login */ },
});
```

The password is sent as `X-Confirm-Password` (not only in a body) so proxies that strip `DELETE` bodies still work. After a successful server delete, the client clears tokens and best-effort clears the local Dexie store so offline data does not remain on the device.

## See also

- [Sync](/app/sync/) — local data wiped on account delete
- [Realtime](/app/realtime/) — WebSocket uses the current access token
