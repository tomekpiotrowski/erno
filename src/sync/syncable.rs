use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement};
use uuid::Uuid;

use crate::{policy::Policy, sync::from_user::FromUser};

/// Builder returned by `Syncable::soft_delete_by_id`, mirroring SeaORM's
/// `delete_by_id` / `DeleteOne` pattern.
///
/// Call `.exec(&db)` to run the update.
pub struct SoftDeleteStatement {
    table: &'static str,
    id: Uuid,
}

impl SoftDeleteStatement {
    /// Execute the soft delete: `UPDATE {table} SET deleted_at = NOW() WHERE id = $1`.
    ///
    /// The per-table sync trigger fires on the UPDATE, stamps a new `sync_seq`,
    /// writes a snapshot to `sync_push_queue`, and notifies the sync listener.
    pub async fn exec(self, db: &DatabaseConnection) -> Result<(), DbErr> {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            &format!("UPDATE {} SET deleted_at = NOW() WHERE id = $1", self.table),
            [self.id.into()],
        ))
        .await?;
        Ok(())
    }
}

/// Marks an entity as participating in offline-first sync.
///
/// Implementing this trait links the entity to a `Policy` that the sync
/// infrastructure uses to determine which connected users should receive
/// WebSocket push events for each change, and enables the `sync_delta`
/// handler factory for per-entity delta sync endpoints.
///
/// Requires `add_sync_columns` to have been called in the entity's migration,
/// which adds `sync_seq` and `deleted_at` columns and the capture trigger.
///
/// # Example
///
/// ```rust,ignore
/// impl Syncable for post::Entity {
///     type Policy = PostPolicy;
///     fn entity_type() -> &'static str { "posts" }
///     fn sync_seq_column() -> post::Column { post::Column::SyncSeq }
///     fn sync_seq(model: &post::Model) -> i64 { model.sync_seq }
/// }
///
/// // Usage in a handler:
/// post::Entity::soft_delete_by_id(id).exec(&app.db).await?;
/// ```
pub trait Syncable: sea_orm::EntityTrait {
    /// The policy that controls read access to this entity.
    /// Must implement `FromUser` so the sync worker can instantiate it per connected user.
    type Policy: Policy<Self> + FromUser + Send + Sync;

    /// The table name, used to key the `SyncRegistry`.
    fn entity_type() -> &'static str;

    /// The typed column for `sync_seq`, used to build `WHERE sync_seq > $since`.
    fn sync_seq_column() -> Self::Column;

    /// Extract the `sync_seq` value from a model instance, used to compute `next_since`.
    fn sync_seq(model: &Self::Model) -> i64;

    /// Soft-delete a record by ID, mirroring SeaORM's `Entity::delete_by_id(id).exec(&db)`.
    ///
    /// Sets `deleted_at = NOW()` via a raw UPDATE. The per-table sync trigger
    /// fires on the UPDATE, stamps `sync_seq`, and propagates the tombstone to
    /// connected clients and delta sync polls.
    ///
    /// # Hard delete vs soft delete
    ///
    /// Hard DELETE (`delete_by_id`) also fires the trigger and pushes a `delete`
    /// event to WebSocket clients that are connected at the time. However, **clients
    /// that are offline will never recover the deletion** — `sync_delta` queries the
    /// entity table directly, and the row is gone. Always use `soft_delete_by_id`
    /// for syncable entities so offline clients can pick up the tombstone on their
    /// next delta pull.
    fn soft_delete_by_id(id: Uuid) -> SoftDeleteStatement {
        SoftDeleteStatement {
            table: Self::entity_type(),
            id,
        }
    }
}
