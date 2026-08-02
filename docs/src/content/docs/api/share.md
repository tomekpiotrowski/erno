---
title: Sharing
description: Share entities with other users via secret links or direct grants, integrated with sync
sidebar:
  order: 15
---

> **Source**: `api/src/share/`

The share module lets a user grant others access to entities they own — via a secret link (no account required) or by granting specific users directly. Shares integrate with [authorization](/api/authorization/) policies and the [sync](/api/sync/) engine: share holders receive both delta pulls and real-time WebSocket push for the shared data.

> **Client counterpart**: [Sharing (App)](/app/share/)

## Concepts

A **share** covers one entity (e.g. one post) and is created by a user with update authority over it. It can carry:

- a **link token** — a 256-bit secret; whoever presents it gets access, no login needed. Only its SHA-256 hash is stored; the raw token is returned exactly once at creation.
- **grants** — owner-issued access for specific users. A grant is active the moment it is created; the recipient is notified (live, if connected), there is no acceptance step.

Access to a shared entity can *imply* access to related entities — a shared post implies its comments. The implication is expressed in your policies (below), not configured in the framework.

All shares are **read-only** in v1. The schema reserves a `permission` column so write shares can be added later.

## The Principal

Authorization for shared access generalizes the policy subject from a user to a `Principal`:

```rust
pub struct Principal {
    pub user: Option<user::Model>,   // None for anonymous link visitors
    pub shares: Vec<ActiveShare>,    // validated, active shares this request/connection holds
}
```

A principal is resolved once per request (or WebSocket connection) from the JWT and any share tokens, then carried as pure data — policy evaluation stays synchronous and database-free, including on the per-event push hot path.

Share link tokens travel in the **`X-Erno-Share` header** (repeatable / comma-separated), never in a query parameter, so they stay out of access logs and `Referer` headers. Share URLs put the token in the fragment (`…/view#s=<token>`), which browsers never send to the server. Authenticated users don't need to send anything: their active grants are loaded into the principal automatically.

## Making an entity shareable

### 1. Implement `FromPrincipal` on its policy

`FromPrincipal` is the share-aware sibling of `FromUser`. Widen `readable`/`can_read` with the principal's shares — and keep `can_update` owner-only, since v1 shares are read-only:

```rust
use erno::share::{FromPrincipal, Principal};

pub struct PostPolicy {
    user_id: Option<Uuid>,
    shared_post_ids: Vec<Uuid>,
}

impl FromPrincipal for PostPolicy {
    fn from_principal(principal: &Principal) -> Self {
        Self {
            user_id: principal.user.as_ref().map(|u| u.id),
            shared_post_ids: principal.shared_ids("posts"),
        }
    }
}

impl Policy<post::Entity> for PostPolicy {
    fn can_read(&self, post: &post::Model) -> bool {
        self.user_id == Some(post.user_id) || self.shared_post_ids.contains(&post.id)
    }

    // IMPORTANT: shares are read-only — do not widen update/delete with shares.
    fn can_update(&self, post: &post::Model) -> bool {
        self.user_id == Some(post.user_id)
    }

    fn readable(&self, query: Select<post::Entity>) -> Select<post::Entity> {
        let mut condition = Condition::any()
            .add(post::Column::Id.is_in(self.shared_post_ids.clone()));
        if let Some(user_id) = self.user_id {
            condition = condition.add(post::Column::UserId.eq(user_id));
        }
        query.filter(condition)
    }
}
```

`can_update` also gates **who may create shares**: sharing an entity requires the same authority as updating it.

### Implied access (post → comments)

A share of a post should expose its comments. Key the comment policy on the *post* ids in the principal:

```rust
impl FromPrincipal for CommentPolicy {
    fn from_principal(principal: &Principal) -> Self {
        Self {
            user_id: principal.user.as_ref().map(|u| u.id),
            shared_post_ids: principal.shared_ids("posts"),  // note: "posts", not "comments"
        }
    }
}

impl Policy<comment::Entity> for CommentPolicy {
    fn can_read(&self, comment: &comment::Model) -> bool {
        self.user_id == Some(comment.user_id)
            || self.shared_post_ids.contains(&comment.post_id)
    }
    // readable: filter on comment::Column::PostId.is_in(shared_post_ids) analogously
}
```

Keep implications narrow — filter children by their parent ids (`is_in`), never by "has any share".

### 2. Register as shareable in BootConfig

```rust
BootConfig::new(app_info, router, job_registry(), job_schedule())
    .with_sync_shared::<post::Entity>()
    .with_sync_shared::<comment::Entity>()
```

`with_sync_shared` requires the policy to implement `FromPrincipal`. Entities registered with plain `with_sync` are never reachable through shares, and only `with_sync_shared` entity types can be shared at all.

### 3. Mount the routers

```rust
Router::new()
    .nest("/shares", share_router(app.clone()))
    .route("/posts/sync", get(sync_delta_shared::<post::Entity, _>))
    .route("/comments/sync", get(sync_delta_shared::<comment::Entity, _>))
    .with_state(app)
```

`sync_delta_shared` is the share-aware variant of `sync_delta`: it builds the policy from the request's principal, so anonymous visitors presenting `X-Erno-Share` and grant recipients (no header needed) receive the shared rows.

## Endpoints

| Method & path | Effect |
|---|---|
| `POST /shares` | Create a share: `{ entity_type, entity_id, link?, recipient_user_ids?, expires_at? }`. Returns the raw link token **once**. |
| `GET /shares?entity_type=&entity_id=` | List own shares with grants (`has_link` flag; hashes never returned). |
| `POST /shares/{id}/grants` | Add a recipient: `{ user_id }`. Active immediately. |
| `DELETE /shares/{id}` | Revoke the whole share (link + all grants). |
| `DELETE /shares/{id}/grants/{user_id}` | Revoke one grant; the link and other grants stay live. |

All endpoints require a JWT. Creating a share is authorized through the entity's policy (`can_update`).

## Real-time behaviour

Shares are live on open WebSocket connections — no reconnect needed:

- **Granting**: if the recipient is connected, the share is fanned into their connection's principal immediately and they receive a `share-granted` broadcast. Subsequent changes to the shared entity (and implied children) push to them in real time.
- **Anonymous link viewers** connect without a JWT and attach the share with a `subscribe-share` control message (`{ "type": "request", "id": "...", "request": { "type": "subscribe-share", "token": "..." } }`). The token rides the message body, never the upgrade URL. `unsubscribe-share` detaches it.
- **Revoking** strips the share from every affected connection and sends a `share-revoked` broadcast; push delivery for it stops at once.
- Push filtering is per-connection: the sync listener evaluates each change event against each connection's principal in memory.

## Client (Angular)

`ErnoShareService` manages shares (create/list/grant/revoke, plus `buildShareUrl` / `tokenFromLocation` fragment helpers). `ErnoSharedViewService` consumes them as an **online-only view**: share-scoped delta pulls and live push land in an in-memory store that is dropped when the view closes or the share is revoked — shared rows never enter the durable per-user IndexedDB store, so the local offline dataset stays clean and revocation needs no cleanup.

```ts
sharedView.registerEntity('posts', '/api/posts/sync');
sharedView.registerEntity('comments', '/api/comments/sync');

const token = shareService.tokenFromLocation();   // reads #s=<token>
const sub = await sharedView.open(token);          // subscribe + pull
sharedView.items$('comments').subscribe(rows => ...);
await sharedView.close(sub.share_id);
```

## Security notes

- Link tokens are 43 base-62 characters (~256 bits), stored only as SHA-256 hashes, looked up by hash via a unique index.
- Tokens never appear in URLs server-side: fragment in links, `X-Erno-Share` header for REST, `subscribe-share` message for WebSocket.
- Shares are read-only; write authority must not be derived from shares in your policies.
- A share token must never widen access to the `shares` / `share_grants` tables themselves.
- Expired (`expires_at`) and revoked shares are filtered at resolution time; long-lived WebSocket connections drop revoked shares via the live fan-out. Expiry of a share held by an already-open connection is not re-checked until reconnect — revoke explicitly if immediate cut-off matters.
