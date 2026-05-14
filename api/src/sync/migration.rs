use sea_orm::ConnectionTrait;

/// Add sync infrastructure columns and trigger to an entity table.
///
/// Call this in an entity migration's `up()` method after creating the table.
/// Adds:
/// - `sync_seq BIGINT NOT NULL DEFAULT 0` — updated atomically by the trigger
/// - `deleted_at TIMESTAMP NULL` — set by `app.soft_delete()` for tombstones
/// - An index on `sync_seq` for efficient delta queries
/// - A `BEFORE INSERT OR UPDATE OR DELETE` trigger that:
///   - Assigns `NEW.sync_seq = nextval('erno_sync_clock')`
///   - Writes a row to `sync_push_queue` (the queue table's own AFTER INSERT trigger fires NOTIFY)
///
/// # Constraints
///
/// - The snapshot is captured via `row_to_json(NEW)` inside a BEFORE trigger.
///   Changes made by AFTER triggers on the same table are not reflected in the snapshot.
/// - Hard DELETE fires the trigger and notifies connected WebSocket clients, but
///   offline clients cannot recover the deletion via `sync_delta` (the row is gone).
///   Use `Syncable::soft_delete_by_id` for all syncable entities.
///
/// # Example (in a SeaORM migration)
///
/// ```rust,ignore
/// async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
///     manager.create_table(...).await?;
///     add_sync_columns(manager, "posts").await?;
///     Ok(())
/// }
/// ```
pub async fn add_sync_columns(
    manager: &sea_orm_migration::SchemaManager<'_>,
    table: &str,
) -> Result<(), sea_orm::DbErr> {
    let conn = manager.get_connection();

    conn.execute_unprepared(&format!(
        "ALTER TABLE {table}
           ADD COLUMN IF NOT EXISTS sync_seq   BIGINT    NOT NULL DEFAULT 0,
           ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMP NULL"
    ))
    .await?;

    conn.execute_unprepared(&format!(
        "CREATE INDEX IF NOT EXISTS idx_{table}_sync_seq ON {table}(sync_seq)"
    ))
    .await?;

    conn.execute_unprepared(&format!(
        r#"
        CREATE OR REPLACE FUNCTION erno_sync_capture_{table}()
        RETURNS trigger AS $$
        DECLARE seq BIGINT;
        BEGIN
            seq := nextval('erno_sync_clock');
            IF TG_OP = 'DELETE' THEN
                INSERT INTO sync_push_queue (entity_type, entity_id, sync_seq, operation, snapshot)
                VALUES (TG_TABLE_NAME, OLD.id, seq, 'delete', row_to_json(OLD)::jsonb);
                RETURN OLD;
            ELSE
                NEW.sync_seq := seq;
                INSERT INTO sync_push_queue (entity_type, entity_id, sync_seq, operation, snapshot)
                VALUES (TG_TABLE_NAME, NEW.id, seq, lower(TG_OP), row_to_json(NEW)::jsonb);
                RETURN NEW;
            END IF;
        END;
        $$ LANGUAGE plpgsql;

        DROP TRIGGER IF EXISTS erno_sync_{table} ON {table};
        CREATE TRIGGER erno_sync_{table}
        BEFORE INSERT OR UPDATE OR DELETE ON {table}
        FOR EACH ROW EXECUTE FUNCTION erno_sync_capture_{table}();
        "#
    ))
    .await?;

    Ok(())
}

/// Reverse `add_sync_columns`: drops the trigger, function, and columns.
///
/// Call this in an entity migration's `down()` method.
pub async fn remove_sync_columns(
    manager: &sea_orm_migration::SchemaManager<'_>,
    table: &str,
) -> Result<(), sea_orm::DbErr> {
    let conn = manager.get_connection();

    conn.execute_unprepared(&format!(
        "DROP TRIGGER IF EXISTS erno_sync_{table} ON {table}"
    ))
    .await?;

    conn.execute_unprepared(&format!(
        "DROP FUNCTION IF EXISTS erno_sync_capture_{table}()"
    ))
    .await?;

    conn.execute_unprepared(&format!(
        "ALTER TABLE {table}
           DROP COLUMN IF EXISTS sync_seq,
           DROP COLUMN IF EXISTS deleted_at"
    ))
    .await?;

    Ok(())
}
