---
title: Realtime
description: ErnoRealtimeService — WebSocket push and background/foreground handling
sidebar:
  order: 4
---

`ErnoRealtimeService` maintains a WebSocket connection to the Erno backend and surfaces incoming sync push events. It reconnects automatically and suspends itself when the app is backgrounded or the device is offline.

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

If the socket drops while a connection is desired, the service reconnects after 3 seconds. Reconnection only happens while `connect()` is in effect (it stops after `disconnect()`), the app is in the foreground, and the device is online.

## Background and foreground

On Capacitor native platforms the app is suspended when backgrounded, which leaves a stale WebSocket. `ErnoRealtimeService` listens to app state via [`ErnoAppStateService`](#app-state) and:

- **On background** — closes the socket and cancels any pending reconnect.
- **On foreground resume** — reopens the socket, but only if `connect()` had previously been called and the device is online.

`ErnoSyncService` complements this by pulling a delta on resume to catch up on events missed while suspended.

## Offline and online

Going offline leaves the same stale socket and would otherwise spin the 3s reconnect loop. `ErnoRealtimeService` listens to connectivity via [`ErnoNetworkService`](#network) and:

- **On offline** — closes the socket and cancels any pending reconnect (without forgetting that the consumer still wants a connection).
- **On online** — reopens the socket when `connect()` is still in effect and the app is in the foreground.

`ErnoSyncService` sets `status$` to `'offline'` while disconnected, skips delta pulls until connectivity returns, and pulls a catch-up delta on the online transition.

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

## Network

`ErnoNetworkService` reports whether the device currently has a network path (`connected`). On native platforms it wraps the optional `@capacitor/network` peer; everywhere else it uses `navigator.onLine` and the browser `online` / `offline` events.

```typescript
import { ErnoNetworkService } from 'erno-angular';

constructor(private network: ErnoNetworkService) {}

ngOnInit() {
  this.network.connected$.subscribe(online => console.log('online?', online));
  this.network.offline$.subscribe(() => console.log('went offline'));
  this.network.online$.subscribe(() => console.log('back online'));
}
```

| Member | Description |
|--------|-------------|
| `connected` / `connected$` | Current boolean connectivity, with an observable that replays the latest value. |
| `online$` | Fires on each offline → online transition. |
| `offline$` | Fires on each online → offline transition. |
| `notifyStatusChange(connected)` | Manually push a status change (used internally by the platform listeners; useful in tests). |

`@capacitor/network` is an **optional** peer dependency — install it in Capacitor apps for accurate native status; web-only consumers need not add it.
