---
title: Sync an entity end-to-end
description: Make a model offline-first on the API and consume it from erno-angular
---

This guide walks through one entity — a `Todo` — from PostgreSQL to IndexedDB: migration, policy, sync registration, API routes, and the Angular client.

For deep reference see [Sync (API)](/api/sync/) and [Sync (App)](/app/sync/).

## 1. Migration with sync columns

```rust
use erno::sync::migration::add_sync_columns;
// in up():
manager.create_table(
    Table::create()
        .table(Todo::Table)
        .col(ColumnDef::new(Todo::Id).uuid().primary_key())
        .col(ColumnDef::new(Todo::UserId).uuid().not_null())
        .col(ColumnDef::new(Todo::Title).text().not_null())
        .to_owned(),
).await?;

add_sync_columns(manager, "todos").await?;
```

This adds `sync_seq`, `deleted_at`, indexes, and the trigger that stamps every change.

## 2. Policy (who can read)

```rust
use erno::policy::Policy;
use sea_orm::{QueryFilter, Select};

pub struct TodoPolicy {
    pub user_id: Uuid,
}

impl Policy<todo::Entity> for TodoPolicy {
    fn can_read(&self, row: &todo::Model) -> bool {
        row.user_id == self.user_id
    }

    fn readable(&self, query: Select<todo::Entity>) -> Select<todo::Entity> {
        query.filter(todo::Column::UserId.eq(self.user_id))
    }
}

// FromUser so sync can build a policy per connected user — see Authorization docs
```

See [Authorization](/api/authorization/) for `FromUser` / full policy setup.

## 3. Implement `Syncable` and register

```rust
use erno::sync::syncable::Syncable;

impl Syncable for todo::Entity {
    type Policy = TodoPolicy;

    fn entity_type() -> &'static str { "todos" }
    fn sync_seq_column() -> todo::Column { todo::Column::SyncSeq }
    fn sync_seq(model: &todo::Model) -> i64 { model.sync_seq }
}
```

In boot:

```rust
BootConfig::new(app_info, router, job_registry(), job_schedule())
    .with_sync::<todo::Entity>()
```

## 4. Mutate with soft delete

Always soft-delete syncable rows so offline clients receive tombstones:

```rust
todo::Entity::soft_delete_by_id(id).exec(&app.db).await?;
```

Hard `DELETE` notifies live sockets but offline clients never see the removal on delta pull.

## 5. Client: register and start

```typescript
import { ErnoSyncService } from 'erno-angular';

// App-owned Dexie store for domain rows (ErnoDatabaseService only holds
// sync cursors + pending mutations).
constructor(private sync: ErnoSyncService, private todosDb: TodosDatabase) {}

async ngOnInit() {
  // deltaPath must match the route you mounted with sync_delta / sync_delta_shared.
  this.sync.register('todos', '/api/todos/sync', async item => {
    if (item.deleted) {
      await this.todosDb.todos.delete(item.id);
      return;
    }
    await this.todosDb.todos.put(item.data as TodoRow);
  });

  await this.sync.start();
}
```

`start()` connects the WebSocket, applies push events through your handler, and pulls deltas for every registered entity. On foreground resume, another pull runs automatically ([Realtime](/app/realtime/)).

## 6. Verify the loop

1. Create a todo via your API while the app is open → push updates the list.
2. Kill the network, create more todos server-side (or from another device), reconnect → `pullDelta` catches up.
3. Soft-delete a row → clients remove it after push or next delta.

Use `<erno-devtools>` to watch sync status and force a re-sync ([Devtools](/app/devtools/)).

## Sharing (optional)

To allow secret-link viewers to receive the same entity:

- Register with `.with_sync_shared::<todo::Entity>()` and a policy that implements `FromPrincipal`
- Mount share-aware delta routes
- On the client, use `ErnoSharedViewService` instead of writing into the personal IndexedDB ([Sharing (App)](/app/share/))

## Checklist

| Step | Done when |
|------|-----------|
| Migration | `sync_seq` + `deleted_at` on table |
| Policy | `readable` scopes rows to the owner |
| `Syncable` + `with_sync` | Entity in registry |
| Soft deletes only | No hard delete on this table |
| Client `register` + `start` | Local store tracks server |
