---
title: Sync
description: ErnoSyncService and ErnoDatabaseService — offline-first delta sync
sidebar:
  order: 3
---

`ErnoSyncService` keeps a local Dexie (IndexedDB) store in sync with the Erno backend. It applies realtime push events as they arrive and pulls deltas to catch up after being offline. `ErnoDatabaseService` is the underlying Dexie wrapper it writes through.

## Usage

Register a handler per entity, then start syncing:

```typescript
import { ErnoSyncService } from 'erno-angular';

constructor(private sync: ErnoSyncService) {}

async ngOnInit() {
  // Path is the app-mounted sync_delta route (under /api after Erno nests the app router).
  this.sync.register('todos', '/api/todos/sync', async item => {
    // item: { entity, id, sync_seq, deleted, data }
    await this.applyToLocalStore(item);
  });

  await this.sync.start();
  this.sync.status$.subscribe(status => console.log('sync', status));
}
```

| Member | Description |
|--------|-------------|
| `register(entity, deltaPath, handler)` | Registers a handler for one entity. `deltaPath` is the absolute path of the delta endpoint (e.g. `/api/todos/sync`). Call before `start()`. |
| `start()` | Connects realtime, subscribes to push events, and runs the initial pull. Idempotent — calling it more than once is a no-op. |
| `pullDelta()` | Fetches and applies the delta for every registered entity (`GET {deltaPath}?since=N` → `{ items, next_since }`). Concurrent calls share a single in-flight request. |
| `status$` | Observable of sync status: `idle` \| `syncing` \| `synced` \| `offline` \| `error`. |

## Background and foreground

When the app returns to the foreground (see [`ErnoAppStateService`](/app/realtime/#app-state)), `ErnoSyncService` automatically calls `pullDelta()` to pick up anything that changed while the app was suspended and the WebSocket was closed. The realtime socket itself is reconnected by [`ErnoRealtimeService`](/app/realtime/).

`pullDelta()` is guarded against overlap, so a resume that lands during an in-flight pull reuses the existing request rather than issuing a second one.

## See also

- [Sync (API)](/api/sync/) — migrations, `Syncable`, soft delete
- [Sync an entity end-to-end](/guides/sync-an-entity/) — full walkthrough
