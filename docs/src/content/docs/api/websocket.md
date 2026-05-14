---
title: WebSocket
description: WebSocket connection management with per-user channels
sidebar:
  order: 7
---

> **Source**: `api/src/websocket/`

Erno's WebSocket layer manages authenticated connections per user and exposes a simple API for broadcasting messages.

## Connection management

`Connections` tracks multiple WebSocket connections per user, identified by `UserId` (UUID). Each connection gets a unique `ConnectionId`.

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

## Mounting the WebSocket route

Mount the built-in WebSocket router to accept connections:

```rust
use erno::websocket;

fn router(app: App) -> Router {
    Router::new()
        .nest("/ws", websocket::router(app.connections.clone()))
        .with_state(app)
}
```

Clients connect to `/ws` with a valid Bearer token in the `Authorization` header. The connection is rejected with 401 if the token is invalid.

## Sending messages to users

```rust
// Send to all connections of a specific user
app.connections.send_to_user(user_id, message_json).await;

// Broadcast to all connected users
app.connections.broadcast(message_json).await;
```

Messages are JSON strings. Structure them however your frontend expects.

## Message format

Erno defines a simple request/response envelope:

```json
// Client → Server
{ "type": "ping", "payload": {} }

// Server → Client
{ "type": "pong", "payload": {} }
```

Implement the `AppRequestHandler` to handle application-specific message types beyond the built-in ping/pong.

## Sync integration

WebSocket connections are the transport layer for Erno's sync system, which pushes database change events to connected clients in real time. See the sync module for details.
