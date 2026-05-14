use std::sync::{Arc, Mutex};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncOperation {
    Insert,
    Update,
    Delete,
}

impl SyncOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncEvent {
    pub entity_type: String,
    pub entity_id: Uuid,
    pub operation: SyncOperation,
    pub snapshot: serde_json::Value,
}

/// Queue for recording entity change events used by delta sync and WebSocket push.
#[derive(Clone, Debug)]
pub enum SyncQueue {
    Database,
    Mock(Arc<Mutex<Vec<SyncEvent>>>),
}

impl Default for SyncQueue {
    fn default() -> Self {
        Self::database()
    }
}

impl SyncQueue {
    pub fn database() -> Self {
        Self::Database
    }

    pub fn mock() -> Self {
        Self::Mock(Arc::new(Mutex::new(Vec::new())))
    }

    /// Record an entity change event. The `sync_seq` is assigned atomically from
    /// the `erno_sync_clock` PostgreSQL sequence; a NOTIFY wakes the push listener.
    pub async fn push(&self, db: &DatabaseConnection, event: SyncEvent) -> Result<(), DbErr> {
        match self {
            Self::Database => {
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    r#"INSERT INTO sync_push_queue
                           (id, entity_type, entity_id, sync_seq, operation, snapshot)
                       VALUES
                           (gen_random_uuid(), $1, $2, nextval('erno_sync_clock'), $3, $4)"#,
                    [
                        event.entity_type.into(),
                        event.entity_id.into(),
                        event.operation.as_str().into(),
                        event.snapshot.into(),
                    ],
                ))
                .await?;
                Ok(())
            }
            Self::Mock(events) => {
                events.lock().unwrap().push(event);
                Ok(())
            }
        }
    }

    pub fn captured_events(&self) -> Option<Vec<SyncEvent>> {
        match self {
            Self::Mock(events) => Some(events.lock().unwrap().clone()),
            Self::Database => None,
        }
    }

    pub fn clear_captured_events(&self) {
        if let Self::Mock(events) = self {
            events.lock().unwrap().clear();
        }
    }
}
