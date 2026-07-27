---
title: WebSocket
description: WebSocket connection management with per-principal channels
sidebar:
  order: 7
---

> **Source**: `api/src/websocket/`

Erno's WebSocket layer manages connections — each authenticated as a [`Principal`](../share) (an optional user plus any active shares) — and exposes a simple API for broadcasting messages.

## Connection management

`Connections` tracks multiple WebSocket connections, each identified by a unique `ConnectionId` and carrying the `Principal` it authenticated as. A single user can have several live connections (e.g. multiple tabs/devices); a connection can also be anonymous (an unauthenticated share-link visitor).

```rust
use erno::websocket::connections::Connections;

// Create the connection store (usually done once at startup)
let connections = Connections::new();

// Or with a custom request handler
let connections = Connections::with_app_handler(|payload| {
    // Handle incoming messages from clients
    Response { /* ... */ }
});
```

## The `/ws` route

`boot()` mounts `/ws` automatically (via `authenticated_ws_handler`) — you don't nest it yourself.

Clients connect with a Bearer token as the `token` query parameter or in the `Authorization` header:

- A **valid** token resolves the connection's `Principal` to that user plus their active share grants; an **invalid** token is rejected with 401.
- **No token** connects anonymously — an empty principal that receives nothing until it attaches a share via the `subscribe-share` control message (see [Sharing](../share)). This is how anonymous share-link visitors connect.

## Sending messages

```rust
// Send to all connections of a specific user
app.websocket_connections.send_to_user(user_id, message_json).await;

// Send to all connections of authenticated users (anonymous/share-link
// connections are excluded — see `send_to_all` docs)
app.websocket_connections.send_to_all(message_json).await;
```

Messages are JSON strings. Structure them however your frontend expects.

## Message format

Erno defines a request/response/broadcast envelope (`api/src/websocket/message.rs`):

```json
// Client → Server
{ "type": "request", "id": "1", "request": { "type": "version" } }

// Server → Client
{ "type": "response", "id": "1", "response": { "type": "version", "version": "..." } }
```

Built-in request types include `version` and the share-related `subscribe-share` / `unsubscribe-share` (see [Sharing](../share)). Implement the `AppRequestHandler` to handle the `application` request variant for your own message types; broadcasts include `share-granted` / `share-revoked` alongside your own `application` broadcasts.

## Sync integration

WebSocket connections are the transport layer for Erno's sync system, which pushes database change events to connected clients in real time. Delivery is evaluated per connection against its `Principal`, so shared entities are pushed to share holders the same way owned entities are pushed to their owner. See the sync module for details.
