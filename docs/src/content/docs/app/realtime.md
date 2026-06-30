---
title: Realtime
description: ErnoRealtimeService — WebSocket push and background/foreground handling
sidebar:
  order: 4
---

`ErnoRealtimeService` maintains a WebSocket connection to the Erno backend and surfaces incoming sync push events. It reconnects automatically and suspends itself when the app is backgrounded.

## Usage

You rarely call this service directly — `ErnoSyncService.start()` connects it for you. To consume raw push events:

```typescript
import { ErnoRealtimeService } from 'erno-angular';

constructor(private realtime: ErnoRealtimeService) {}

ngOnInit() {
  this.realtime.connect();
  this.realtime.events$.subscribe(event => {
    // { entity, id, sync_seq, deleted }
    console.log('push', event);
  });
}
```

| Member | Description |
|--------|-------------|
| `connect()` | Opens the WebSocket using the current access token. Records the intent to stay connected. |
| `disconnect()` | Closes the socket and stops reconnecting. |
| `events$` | Observable of incoming `SyncPushEvent`s. |

## Reconnection

If the socket drops while a connection is desired, the service reconnects after 3 seconds. Reconnection only happens while `connect()` is in effect (it stops after `disconnect()`) and while the app is in the foreground.

## Background and foreground

On Capacitor native platforms the app is suspended when backgrounded, which leaves a stale WebSocket. `ErnoRealtimeService` listens to app state via [`ErnoAppStateService`](#app-state) and:

- **On background** — closes the socket and cancels any pending reconnect.
- **On foreground resume** — reopens the socket, but only if `connect()` had previously been called.

`ErnoSyncService` complements this by pulling a delta on resume to catch up on events missed while suspended.

## App state

`ErnoAppStateService` reports whether the app is in the foreground (`active`) or `background`. It wraps the optional `@capacitor/app` peer dependency on native platforms and falls back to the browser `visibilitychange` event everywhere else, so it works in plain web apps without Capacitor installed.

```typescript
import { ErnoAppStateService } from 'erno-angular';

constructor(private appState: ErnoAppStateService) {}

ngOnInit() {
  this.appState.resumed$.subscribe(() => console.log('app foregrounded'));
  this.appState.paused$.subscribe(() => console.log('app backgrounded'));
}
```

| Member | Description |
|--------|-------------|
| `state` / `state$` | Current state (`'active'` \| `'background'`), with an observable that replays the latest value. |
| `resumed$` | Fires on each background → active transition. |
| `paused$` | Fires on each active → background transition. |
| `notifyStateChange(state)` | Manually push a state change (used internally by the platform listeners; useful in tests). |

`@capacitor/app` is an **optional** peer dependency — the `erno new` scaffold installs it for native apps, and web-only consumers need not add it.
