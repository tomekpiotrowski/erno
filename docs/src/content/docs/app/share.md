---
title: Sharing
description: ErnoShareService and ErnoSharedViewService — links, grants, and online shared views
sidebar:
  order: 7
---

Sharing has two client surfaces:

1. **`ErnoShareService`** — owner-side management (create link/grant shares, list, revoke).
2. **`ErnoSharedViewService`** — consumer-side online view of shared data (in-memory, not IndexedDB).

> **Backend counterpart**: [Sharing (API)](/api/share/)

## Owner: create and manage shares

```typescript
import { ErnoShareService } from 'erno-angular';

constructor(private shares: ErnoShareService) {}

createLink(postId: string) {
  this.shares.create({
    entity_type: 'posts',
    entity_id: postId,
    link: true,
  }).subscribe(res => {
    // res.token is returned exactly once when link: true
    const url = this.shares.buildShareUrl(
      `${window.location.origin}/shared/post`,
      res.token!,
    );
    // show url to the user
  });
}

grantToUsers(postId: string, userIds: string[]) {
  this.shares.create({
    entity_type: 'posts',
    entity_id: postId,
    recipient_user_ids: userIds,
  }).subscribe();
}
```

| Member | Description |
|--------|-------------|
| `create(request)` | `POST /api/shares` — optional `link`, `recipient_user_ids`, `expires_at` |
| `list(filter?)` | `GET /api/shares` — optional `entity_type` / `entity_id` |
| `addGrant(shareId, userId)` | `POST /api/shares/:id/grants` |
| `revoke(shareId)` | `DELETE /api/shares/:id` |
| `revokeGrant(shareId, userId)` | `DELETE /api/shares/:id/grants/:userId` |
| `buildShareUrl(viewUrl, token)` | Puts the raw token in the URL **fragment** (`#s=...`) |
| `tokenFromLocation(hash?)` | Reads `#s=` from `window.location.hash` |

### Fragment tokens

Share link tokens ride the **URL fragment** so they are not sent to your HTTP server as a path or query (and stay out of access logs / `Referer`). When the viewer opens the page, call `tokenFromLocation()` and pass the token to `ErnoSharedViewService.open` or attach it with the `X-Erno-Share` header (`SHARE_TOKEN_HEADER`).

## Consumer: online shared view

`ErnoSharedViewService` holds shared rows **in memory only**. That keeps the owner’s offline IndexedDB clean and makes revocation a simple drop of the view.

```typescript
import { ErnoSharedViewService } from 'erno-angular';

constructor(
  private sharedView: ErnoSharedViewService,
  private shares: ErnoShareService,
) {}

async ngOnInit() {
  // Delta paths must be share-aware on the server (sync_delta_shared)
  this.sharedView.registerEntity('posts', '/api/posts/sync');
  this.sharedView.registerEntity('comments', '/api/comments/sync');

  const token = this.shares.tokenFromLocation();
  if (!token) return;

  await this.sharedView.open(token);
  this.sharedView.items$('posts').subscribe(rows => {
    this.posts = rows;
  });
}
```

| Member | Description |
|--------|-------------|
| `registerEntity(entity, deltaPath)` | Map entity name → share-aware delta URL path |
| `open(token)` | Subscribe the WebSocket to the share + pull deltas |
| `pull(shareId)` | Re-fetch deltas (e.g. after reconnect) |
| `items$(entity)` | Observable of current in-memory rows for that entity |

Realtime: `open` uses `ErnoRealtimeService.subscribeShare(token)` so live push applies to the shared view. See [Realtime](/app/realtime/).

## Server requirements

- Entities reachable through shares must be registered with `with_sync_shared` and policies that implement `FromPrincipal`.
- Mount share-aware delta handlers; requests carry `X-Erno-Share: <raw token>`.
