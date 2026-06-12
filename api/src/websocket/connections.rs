use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures_util::future::BoxFuture;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::share::principal::{ActiveShare, Principal};
use crate::websocket::message::{Broadcast, Message as WsMessage, Request, Response};

pub type ConnectionId = Uuid;
pub type UserId = Uuid;
pub type ConnectionSender = mpsc::UnboundedSender<String>;
pub type AppRequestHandler = Arc<dyn Fn(Value) -> Response + Send + Sync>;

/// Validates a raw share link token (sent via the `subscribe-share` control
/// message) and resolves it to an active share. Constructed by the WebSocket
/// upgrade handler, which captures the database connection.
pub type ShareTokenValidator =
    Arc<dyn Fn(String) -> BoxFuture<'static, Option<ActiveShare>> + Send + Sync>;

struct ConnectionEntry {
    principal: Principal,
    sender: ConnectionSender,
}

#[derive(Clone)]
pub struct Connections {
    // Each connection carries the Principal it authenticated as. The principal
    // is mutated live on share grant/revoke/subscribe so push filtering always
    // reflects current access without reconnecting.
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionEntry>>>,
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
        for (connection_id, entry) in connections.iter() {
            if entry.principal.user.as_ref().map(|u| u.id) == Some(user_id) {
                if let Err(e) = entry.sender.send(message.clone()) {
                    error!(
                        "Failed to send message to user {} connection {}: {:?}",
                        user_id, connection_id, e
                    );
                }
            }
        }
    }

    /// Send a message to all connections of authenticated users.
    ///
    /// Anonymous connections (share-link viewers) are excluded: app-wide
    /// broadcasts are addressed to users, and a share token only scopes
    /// access to the shared entities.
    pub async fn send_to_all(&self, message: String) {
        let connections = self.connections.lock().await;
        for (connection_id, entry) in connections.iter() {
            if entry.principal.user.is_none() {
                continue;
            }
            if let Err(e) = entry.sender.send(message.clone()) {
                error!(
                    "Failed to send message to connection {}: {:?}",
                    connection_id, e
                );
            }
        }
    }

    /// Get the IDs of all currently connected (authenticated) users.
    pub async fn connected_user_ids(&self) -> Vec<Uuid> {
        let connections = self.connections.lock().await;
        let ids: HashSet<Uuid> = connections
            .values()
            .filter_map(|e| e.principal.user.as_ref().map(|u| u.id))
            .collect();
        ids.into_iter().collect()
    }

    /// Get count of connected (authenticated) users
    pub async fn user_count(&self) -> usize {
        self.connected_user_ids().await.len()
    }

    /// Get total count of connections (including anonymous ones)
    pub async fn connection_count(&self) -> usize {
        self.connections.lock().await.len()
    }

    /// Snapshot of all connections with their current principals.
    ///
    /// Used by the sync push listener to evaluate, per change event, which
    /// connections may read the changed entity. Principals carry pre-resolved
    /// shares, so the per-event evaluation stays in-memory.
    pub async fn snapshot(&self) -> Vec<(ConnectionId, Principal, ConnectionSender)> {
        let connections = self.connections.lock().await;
        connections
            .iter()
            .map(|(id, entry)| (*id, entry.principal.clone(), entry.sender.clone()))
            .collect()
    }

    /// Forcibly close all of a user's WebSocket connections — e.g. on account
    /// deletion, so a deleted user's live sockets don't linger. Dropping the
    /// per-connection senders ends each connection's outgoing task, which
    /// closes the socket.
    pub async fn disconnect_user(&self, user_id: UserId) {
        let mut connections = self.connections.lock().await;
        let ids: Vec<ConnectionId> = connections
            .iter()
            .filter(|(_, entry)| entry.principal.user.as_ref().map(|u| u.id) == Some(user_id))
            .map(|(id, _)| *id)
            .collect();
        let count = ids.len();
        for id in &ids {
            connections.remove(id);
        }
        drop(connections);
        if count > 0 {
            info!(
                "Disconnecting {} WebSocket connection(s) for user {}",
                count, user_id
            );
        }
    }

    /// Register a connection. Used by the socket pump and by tests.
    pub(crate) async fn register_connection(
        &self,
        principal: Principal,
        sender: ConnectionSender,
    ) -> ConnectionId {
        let connection_id = Uuid::new_v4();
        let mut connections = self.connections.lock().await;
        connections.insert(connection_id, ConnectionEntry { principal, sender });
        connection_id
    }

    pub(crate) async fn unregister_connection(&self, connection_id: ConnectionId) {
        self.connections.lock().await.remove(&connection_id);
    }

    /// Attach an active share to a single connection (`subscribe-share`).
    pub async fn subscribe_share(&self, connection_id: ConnectionId, share: ActiveShare) {
        let mut connections = self.connections.lock().await;
        if let Some(entry) = connections.get_mut(&connection_id) {
            if !entry.principal.has_share(share.id) {
                entry.principal.shares.push(share);
            }
        }
    }

    /// Detach a share from a single connection (`unsubscribe-share`).
    pub async fn unsubscribe_share(&self, connection_id: ConnectionId, share_id: Uuid) {
        let mut connections = self.connections.lock().await;
        if let Some(entry) = connections.get_mut(&connection_id) {
            entry.principal.shares.retain(|s| s.id != share_id);
        }
    }

    /// Server-initiated fan-in: attach a freshly granted share to every live
    /// connection of `user_id` and notify them with a `share-granted`
    /// broadcast. The recipient starts receiving push events for the shared
    /// entity without reconnecting.
    pub async fn add_share_to_user(&self, user_id: UserId, share: ActiveShare) -> usize {
        let notification = share_granted_message(&share);
        let mut connections = self.connections.lock().await;
        let mut updated = 0;
        for entry in connections.values_mut() {
            if entry.principal.user.as_ref().map(|u| u.id) != Some(user_id) {
                continue;
            }
            if !entry.principal.has_share(share.id) {
                entry.principal.shares.push(share.clone());
            }
            let _ = entry.sender.send(notification.clone());
            updated += 1;
        }
        updated
    }

    /// Revoke fan-out: strip `share_id` from every connection holding it and
    /// notify those connections with a `share-revoked` broadcast.
    pub async fn remove_share_everywhere(&self, share_id: Uuid) {
        self.remove_share(share_id, None).await;
    }

    /// Single-grant revoke fan-out: strip `share_id` from `user_id`'s
    /// connections only (the link token and other grants stay live).
    pub async fn remove_share_from_user(&self, share_id: Uuid, user_id: UserId) {
        self.remove_share(share_id, Some(user_id)).await;
    }

    async fn remove_share(&self, share_id: Uuid, only_user: Option<UserId>) {
        let notification = share_revoked_message(share_id);
        let mut connections = self.connections.lock().await;
        for entry in connections.values_mut() {
            if let Some(user_id) = only_user {
                if entry.principal.user.as_ref().map(|u| u.id) != Some(user_id) {
                    continue;
                }
            }
            if entry.principal.has_share(share_id) {
                entry.principal.shares.retain(|s| s.id != share_id);
                let _ = entry.sender.send(notification.clone());
            }
        }
    }

    pub async fn handle_socket(
        &self,
        principal: Principal,
        socket: WebSocket,
        share_validator: Option<ShareTokenValidator>,
    ) {
        let user_label = principal
            .user
            .as_ref()
            .map_or_else(|| "anonymous".to_string(), |u| u.id.to_string());

        let (mut sender, mut receiver) = socket.split();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let connection_id = self.register_connection(principal, tx).await;
        info!(
            "🔌 New WebSocket connection: {} for principal: {}",
            connection_id, user_label
        );

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
        let self_clone = self.clone();
        let app_handler = self.app_handler.clone();
        let incoming_task = tokio::spawn(async move {
            // Sliding-window message rate limiter: max 20 messages per second per connection.
            // Exceeding this disconnects the client to prevent message-flood DDoS.
            const MAX_MSGS_PER_WINDOW: usize = 20;
            const RATE_WINDOW: Duration = Duration::from_secs(1);
            let mut msg_timestamps: VecDeque<Instant> = VecDeque::new();

            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        // Enforce per-connection message rate limit
                        let now = Instant::now();
                        msg_timestamps.retain(|&t| now - t < RATE_WINDOW);
                        if msg_timestamps.len() >= MAX_MSGS_PER_WINDOW {
                            warn!(
                                connection_id = %connection_id,
                                "WebSocket message rate limit exceeded, closing connection"
                            );
                            break;
                        }
                        msg_timestamps.push_back(now);

                        if let Ok(WsMessage::Request { request, id }) =
                            serde_json::from_str::<WsMessage>(&text)
                        {
                            let response = self_clone
                                .handle_request(
                                    connection_id,
                                    request,
                                    &app_handler,
                                    &share_validator,
                                )
                                .await;
                            let response_msg = WsMessage::Response { response, id };

                            if let Ok(serialized) = serde_json::to_string(&response_msg) {
                                self_clone
                                    .send_to_connection(connection_id, serialized)
                                    .await;
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

        self.unregister_connection(connection_id).await;
        info!(
            "🔌 WebSocket connection closed: {} for principal: {}",
            connection_id, user_label
        );
    }

    async fn send_to_connection(&self, connection_id: ConnectionId, message: String) {
        let connections = self.connections.lock().await;
        if let Some(entry) = connections.get(&connection_id) {
            let _ = entry.sender.send(message);
        }
    }

    async fn handle_request(
        &self,
        connection_id: ConnectionId,
        request: Request,
        app_handler: &Option<AppRequestHandler>,
        share_validator: &Option<ShareTokenValidator>,
    ) -> Response {
        match request {
            Request::Version => Response::Version {
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            Request::SubscribeShare { token } => {
                let Some(validator) = share_validator else {
                    return Response::Error {
                        error: "Share subscriptions not supported".to_string(),
                    };
                };
                match validator(token).await {
                    Some(share) => {
                        let response = Response::ShareSubscribed {
                            share_id: share.id,
                            entity_type: share.entity_type.clone(),
                            entity_id: share.entity_id,
                        };
                        self.subscribe_share(connection_id, share).await;
                        response
                    }
                    None => Response::Error {
                        error: "Invalid share token".to_string(),
                    },
                }
            }
            Request::UnsubscribeShare { share_id } => {
                self.unsubscribe_share(connection_id, share_id).await;
                Response::Ok
            }
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
}

fn share_granted_message(share: &ActiveShare) -> String {
    serde_json::to_string(&WsMessage::Broadcast {
        broadcast: Broadcast::ShareGranted {
            share_id: share.id,
            entity_type: share.entity_type.clone(),
            entity_id: share.entity_id,
        },
    })
    .expect("share-granted broadcast serializes")
}

fn share_revoked_message(share_id: Uuid) -> String {
    serde_json::to_string(&WsMessage::Broadcast {
        broadcast: Broadcast::ShareRevoked { share_id },
    })
    .expect("share-revoked broadcast serializes")
}
