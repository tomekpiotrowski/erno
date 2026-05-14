use std::sync::Arc;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use sqlx::postgres::PgListener;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::database::models::sync_push_queue::{self, Entity as SyncPushQueue};
use crate::database::models::user::{self, Entity as User};
use crate::sync::registry::SyncRegistry;
use crate::websocket::connections::Connections;
use crate::websocket::message::{Broadcast, Message};

/// Start the sync push listener. Wakes on NOTIFY from the `sync_push_queue`
/// trigger, evaluates which connected users may read each changed entity via
/// the `SyncRegistry`, fans out targeted WebSocket messages, then deletes
/// processed rows from the queue.
pub async fn start_sync_listener(
    db: DatabaseConnection,
    connections: Connections,
    registry: Arc<SyncRegistry>,
) {
    loop {
        if let Err(e) = listen_loop(&db, &connections, &registry).await {
            error!("Sync listener error: {}, restarting in 5s...", e);
        } else {
            warn!("Sync listener exited normally, restarting...");
        }
        sleep(Duration::from_secs(5)).await;
    }
}

async fn listen_loop(
    db: &DatabaseConnection,
    connections: &Connections,
    registry: &SyncRegistry,
) -> Result<(), Box<dyn std::error::Error>> {
    let sqlx_pool = db.get_postgres_connection_pool();

    let mut listener = PgListener::connect_with(sqlx_pool).await?;
    listener.listen("sync_new_event").await?;

    // Start from 0 so any rows left in the queue from a previous crash are reprocessed.
    // This gives at-least-once delivery (duplicate events on crash mid-batch) rather than
    // silent loss.
    let mut last_seq: i64 = 0;

    info!("Sync listener started (watermark seq={})", last_seq);

    loop {
        listener.recv().await?;

        debug!("Sync listener woke, draining events since seq={}", last_seq);

        let events = SyncPushQueue::find()
            .filter(sync_push_queue::Column::SyncSeq.gt(last_seq))
            .order_by_asc(sync_push_queue::Column::SyncSeq)
            .all(db)
            .await?;

        if events.is_empty() {
            continue;
        }

        // Load user models for all currently connected users in one batch query.
        let connected_ids = connections.connected_user_ids().await;
        let users: Vec<user::Model> = if connected_ids.is_empty() {
            vec![]
        } else {
            User::find()
                .filter(user::Column::Id.is_in(connected_ids))
                .all(db)
                .await?
        };

        let mut processed_ids: Vec<Uuid> = Vec::with_capacity(events.len());

        for event in &events {
            let seq = event.sync_seq;

            let payload = serde_json::json!({
                "entity_type": event.entity_type,
                "entity_id":   event.entity_id,
                "sync_seq":    event.sync_seq,
                "operation":   event.operation,
                "snapshot":    event.snapshot,
            });

            let msg = Message::Broadcast {
                broadcast: Broadcast::Application(payload),
            };

            let Ok(serialized) = serde_json::to_string(&msg) else {
                error!("Failed to serialize sync event seq={}", seq);
                last_seq = seq;
                processed_ids.push(event.id);
                continue;
            };

            // Send only to users whose policy permits reading this entity.
            for user in &users {
                if registry.can_user_read(&event.entity_type, &event.snapshot, user) {
                    debug!(
                        "Pushing sync event seq={} to user {} (policy allowed)",
                        seq, user.id
                    );
                    connections.send_to_user(user.id, serialized.clone()).await;
                }
            }

            last_seq = seq;
            processed_ids.push(event.id);
        }

        // Delete processed rows — the queue is transient; delta sync uses entity tables directly.
        if !processed_ids.is_empty() {
            SyncPushQueue::delete_many()
                .filter(sync_push_queue::Column::Id.is_in(processed_ids))
                .exec(db)
                .await?;
        }
    }
}
