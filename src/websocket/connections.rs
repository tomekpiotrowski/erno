use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
use uuid::Uuid;

use crate::websocket::message::{Message as WsMessage, Request, Response};

pub type ConnectionId = Uuid;

#[derive(Clone, Debug)]
pub struct Connections {
    connections: Arc<Mutex<HashMap<ConnectionId, mpsc::UnboundedSender<String>>>>,
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
        }
    }

    pub async fn handle_socket(&self, socket: WebSocket) {
        let connection_id = Uuid::new_v4();
        info!("🔌 New WebSocket connection: {}", connection_id);

        let (mut sender, mut receiver) = socket.split();
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Add connection to manager
        {
            let mut connections = self.connections.lock().await;
            connections.insert(connection_id, tx);
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
        let incoming_task = tokio::spawn(async move {
            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(ws_message) = serde_json::from_str::<WsMessage>(&text) {
                            if let WsMessage::Request { request, id } = ws_message {
                                let response = handle_request(request);
                                let response_msg = WsMessage::Response { response, id };

                                if let Ok(serialized) = serde_json::to_string(&response_msg) {
                                    // Send back through the connection
                                    let connections_guard = connections.lock().await;
                                    if let Some(tx) = connections_guard.get(&connection_id) {
                                        let _ = tx.send(serialized);
                                    }
                                }
                            } else {
                                let error_msg = WsMessage::Error {
                                    message: "Only requests are supported".to_string(),
                                };
                                if let Ok(serialized) = serde_json::to_string(&error_msg) {
                                    let connections_guard = connections.lock().await;
                                    if let Some(tx) = connections_guard.get(&connection_id) {
                                        let _ = tx.send(serialized);
                                    }
                                }
                            }
                        } else {
                            let error_msg = WsMessage::Error {
                                message: "Invalid message format".to_string(),
                            };
                            if let Ok(serialized) = serde_json::to_string(&error_msg) {
                                let connections_guard = connections.lock().await;
                                if let Some(tx) = connections_guard.get(&connection_id) {
                                    let _ = tx.send(serialized);
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
            connections.remove(&connection_id);
        }
        info!("🔌 WebSocket connection closed: {}", connection_id);
    }
}

fn handle_request(request: Request) -> Response {
    match request {
        Request::Version => Response::Version {
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    }
}
