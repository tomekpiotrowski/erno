---
title: Sync
description: Offline-first delta synchronization over WebSocket
sidebar:
  order: 14
---

> **Source**: `api/src/sync/`

Erno's sync module provides offline-first data synchronization. Entities publish changes via a PostgreSQL trigger; connected clients receive real-time WebSocket push events; clients that were offline catch up via a `GET /sync/{entity}/delta?since={seq}` endpoint.

## Making an entity syncable

### 1. Add sync columns in the migration

Call `add_sync_columns` after creating the table in your SeaORM migration:

```rust
use erno::sync::migration::add_sync_columns;

async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.create_table(
        Table::create()
            .table(Post::Table)
            .col(ColumnDef::new(Post::Id).uuid().primary_key())
            .col(ColumnDef::new(Post::UserId).uuid().not_null())
            .col(ColumnDef::new(Post::Body).text().not_null())
            .to_owned(),
    ).await?;

    add_sync_columns(manager, "posts").await?;
    Ok(())
}
```

This adds `sync_seq BIGINT NOT NULL DEFAULT 0`, `deleted_at TIMESTAMP NULL`, an index on `sync_seq`, and a `BEFORE INSERT OR UPDATE OR DELETE` trigger that stamps each change with a monotonic sequence value.

To reverse in `down()`:

```rust
use erno::sync::migration::remove_sync_columns;

remove_sync_columns(manager, "posts").await?;
```

### 2. Implement `Syncable` on the entity

```rust
use erno::sync::syncable::Syncable;

impl Syncable for post::Entity {
    type Policy = PostPolicy;   // must also implement FromUser

    fn entity_type() -> &'static str { "posts" }
    fn sync_seq_column() -> post::Column { post::Column::SyncSeq }
    fn sync_seq(model: &post::Model) -> i64 { model.sync_seq }
}
```

`type Policy` controls which connected users receive push events for a given change — only users for whom `policy.readable(query)` would return the entity are notified. See [Authorization](../authorization) for implementing policies and `FromUser`.

### 3. Register in BootConfig

```rust
BootConfig::new(app_info, router, job_registry(), job_schedule())
    .with_sync::<post::Entity>()
```

This registers the entity with the `SyncRegistry` and wires up the delta sync endpoint and WebSocket listener.

## Soft delete

Always soft-delete syncable entities so clients that were offline can pick up the tombstone on their next delta pull. Use `soft_delete_by_id` instead of SeaORM's `delete_by_id`:

```rust
post::Entity::soft_delete_by_id(post_id).exec(&app.db).await?;
```

This sets `deleted_at = NOW()` via a raw `UPDATE`. The trigger fires, stamps `sync_seq`, and propagates the tombstone to both WebSocket push and delta sync.

**Why not hard DELETE?** A hard delete fires the trigger and notifies currently-connected clients, but the row is gone — clients that are offline will never recover the deletion when they poll `/sync/posts/delta`. Always use soft delete for syncable entities.

## Delta sync endpoint

Once registered, Erno exposes:

```
GET /sync/{entity_type}/delta?since={seq}
Authorization: Bearer <access_token>
```

The client sends the highest `sync_seq` it has seen (or `0` for a full sync). The response is a list of records (including soft-deleted tombstones) with `sync_seq > since`, plus a `next_since` value to use on the next poll.

## How it works

1. Any `INSERT`, `UPDATE`, or `DELETE` on a syncable table fires a PostgreSQL trigger.
2. The trigger stamps `sync_seq` from a global sequence (`erno_sync_clock`) and writes a row to `sync_push_queue`.
3. A NOTIFY fires on `sync_push_queue`; the sync listener picks it up and pushes a WebSocket message to every connected user whose policy includes the changed entity.
4. Offline clients call the delta endpoint on reconnect and catch up from their last known `sync_seq`.
