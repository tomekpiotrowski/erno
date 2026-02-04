use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
use uuid::Uuid;

use crate::websocket::message::{Message as WsMessage, Request, Response};

pub type ConnectionId = Uuid;
pub type UserId = Uuid;
pub type ConnectionSender = mpsc::UnboundedSender<String>;
pub type UserConnections = Vec<(ConnectionId, ConnectionSender)>;
pub type ConnectionStore = Arc<Mutex<HashMap<UserId, UserConnections>>>;
pub type AppRequestHandler = Arc<dyn Fn(Value) -> Response + Send + Sync>;

#[derive(Clone)]
pub struct Connections {
    // Track multiple connections per user: UserId -> Vec<(ConnectionId, Sender)>
    connections: ConnectionStore,
    // Optional application-specific request handler
    app_handler: Option<AppRequestHandler>,
}

impl Default for Connections {
    fn default() -> Self {
        Self::new()
    }
}

impl Connections {
    #[must_use]
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            app_handler: None,
        }
    }

    /// Create a new Connections manager with an application-specific request handler
    #[must_use]
    pub fn with_app_handler<F>(handler: F) -> Self
    where
        F: Fn(Value) -> Response + Send + Sync + 'static,
    {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            app_handler: Some(Arc::new(handler)),
        }
    }

    /// Send a message to all connections for a specific user
    pub async fn send_to_user(&self, user_id: UserId, message: String) {
        let connections = self.connections.lock().await;
        if let Some(user_connections) = connections.get(&user_id) {
            for (connection_id, tx) in user_connections {
                if let Err(e) = tx.send(message.clone()) {
                    error!(
                        "Failed to send message to user {} connection {}: {:?}",
                        user_id, connection_id, e
                    );
                }
            }
        }
    }

    /// Send a message to all connected users
    pub async fn send_to_all(&self, message: String) {
        let connections = self.connections.lock().await;
        for (_user_id, user_connections) in connections.iter() {
            for (connection_id, tx) in user_connections {
                if let Err(e) = tx.send(message.clone()) {
                    error!(
                        "Failed to send message to connection {}: {:?}",
                        connection_id, e
                    );
                }
            }
        }
    }

    /// Get count of connected users
    pub async fn user_count(&self) -> usize {
        self.connections.lock().await.len()
    }

    /// Get total count of connections
    pub async fn connection_count(&self) -> usize {
        self.connections
            .lock()
            .await
            .values()
            .map(|conns| conns.len())
            .sum()
    }

    pub async fn handle_socket(&self, user_id: UserId, socket: WebSocket) {
        let connection_id = Uuid::new_v4();
        info!(
            "🔌 New WebSocket connection: {} for user: {}",
            connection_id, user_id
        );

        let (mut sender, mut receiver) = socket.split();
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Add connection to manager
        {
            let mut connections = self.connections.lock().await;
            connections
                .entry(user_id)
                .or_insert_with(Vec::new)
                .push((connection_id, tx));
        }

        // Handle outgoing messages
        let outgoing_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let Err(e) = sender.send(Message::Text(msg.into())).await {
                    error!("Failed to send WebSocket message: {:?}", e);
                    break;
                }
            }
        });

        // Handle incoming messages
        let connections = self.connections.clone();
        let app_handler = self.app_handler.clone();
        let incoming_task = tokio::spawn(async move {
            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(ws_message) = serde_json::from_str::<WsMessage>(&text) {
                            if let WsMessage::Request { request, id } = ws_message {
                                let response = handle_request(request, &app_handler);
                                let response_msg = WsMessage::Response { response, id };

                                if let Ok(serialized) = serde_json::to_string(&response_msg) {
                                    // Send back through the user's connections
                                    let connections_guard = connections.lock().await;
                                    if let Some(user_connections) = connections_guard.get(&user_id)
                                    {
                                        if let Some((_cid, tx)) = user_connections
                                            .iter()
                                            .find(|(cid, _)| *cid == connection_id)
                                        {
                                            let _ = tx.send(serialized);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(e) => {
                        error!("WebSocket error: {:?}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        // Wait for either task to complete
        tokio::select! {
            _ = outgoing_task => {},
            _ = incoming_task => {},
        }

        // Clean up connection
        {
            let mut connections = self.connections.lock().await;
            if let Some(user_connections) = connections.get_mut(&user_id) {
                user_connections.retain(|(cid, _)| *cid != connection_id);
                // Remove user entry if no more connections
                if user_connections.is_empty() {
                    connections.remove(&user_id);
                }
            }
        }
        info!(
            "🔌 WebSocket connection closed: {} for user: {}",
            connection_id, user_id
        );
    }
}

fn handle_request(request: Request, app_handler: &Option<AppRequestHandler>) -> Response {
    match request {
        Request::Version => Response::Version {
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        Request::Application(value) => {
            if let Some(handler) = app_handler {
                handler(value)
            } else {
                Response::Error {
                    error: "Application requests not supported".to_string(),
                }
            }
        }
    }
}
