use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgListener;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::database::models::websocket_message::Entity as WebsocketMessage;
use crate::websocket::connections::{Connections, UserId};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecipientCriteria {
    /// Send to a specific user
    User { user_id: UserId },
    /// Send to all connected users
    All,
}

/// Start listening for PostgreSQL NOTIFY events and broadcast messages to WebSocket connections
pub async fn start_listener(db: DatabaseConnection, connections: Connections) {
    // Only start listener for PostgreSQL databases
    if !matches!(db.get_database_backend(), DatabaseBackend::Postgres) {
        info!("WebSocket listener not started: database is not PostgreSQL");
        return;
    }

    loop {
        if let Err(e) = listen_loop(&db, &connections).await {
            error!("WebSocket listener error: {}, restarting in 5s...", e);
        } else {
            warn!("WebSocket listener exited normally, restarting...");
        }
        sleep(Duration::from_secs(5)).await;
    }
}

async fn listen_loop(
    db: &DatabaseConnection,
    connections: &Connections,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get the underlying sqlx pool from SeaORM
    let sqlx_pool = db.get_postgres_connection_pool();

    let mut listener = PgListener::connect_with(sqlx_pool).await?;
    listener.listen("websocket_new_message").await?;

    info!("WebSocket listener started, listening on channel 'websocket_new_message'");

    loop {
        let notification = listener.recv().await?;
        let message_id = notification.payload();

        info!("Received notification for message_id: {}", message_id);

        // Parse the message ID
        let message_id = match Uuid::parse_str(message_id) {
            Ok(id) => id,
            Err(e) => {
                error!("Failed to parse message_id '{}': {:?}", message_id, e);
                continue;
            }
        };

        // Fetch the message from the database
        let message = match WebsocketMessage::find_by_id(message_id).one(db).await {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                warn!("Message {} not found in database", message_id);
                continue;
            }
            Err(e) => {
                error!("Failed to fetch message {}: {:?}", message_id, e);
                continue;
            }
        };

        // Parse recipient criteria
        let criteria: RecipientCriteria = match serde_json::from_value(message.recipient_criteria) {
            Ok(c) => c,
            Err(e) => {
                error!(
                    "Failed to parse recipient_criteria for message {}: {:?}",
                    message_id, e
                );
                continue;
            }
        };

        // Convert payload to string for sending
        let payload = match serde_json::to_string(&message.payload) {
            Ok(p) => p,
            Err(e) => {
                error!(
                    "Failed to serialize payload for message {}: {:?}",
                    message_id, e
                );
                continue;
            }
        };

        // Broadcast based on criteria
        match criteria {
            RecipientCriteria::User { user_id } => {
                info!("Sending message to user {}", user_id);
                connections.send_to_user(user_id, payload).await;
            }
            RecipientCriteria::All => {
                info!("Broadcasting message to all users");
                connections.send_to_all(payload).await;
            }
        }

        // Delete the message after processing
        if let Err(e) = WebsocketMessage::delete_by_id(message_id).exec(db).await {
            error!("Failed to delete message {}: {:?}", message_id, e);
        }
    }
}
