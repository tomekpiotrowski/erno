use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    Version,
    /// Attach an active share to this connection. The raw link token is sent
    /// here — post-connect, over the socket — rather than in the upgrade URL,
    /// so it never lands in access logs or browser history.
    SubscribeShare { token: String },
    /// Detach a share from this connection (e.g. the shared view was closed).
    UnsubscribeShare { share_id: Uuid },
    /// Application-specific requests
    /// The Value should be an object with a "type" field for routing
    Application(Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Response {
    Version {
        version: String,
    },
    Ok,
    Error {
        error: String,
    },
    /// Successful `SubscribeShare`: the connection now receives push events
    /// covered by this share. `share_id` is the handle for `UnsubscribeShare`.
    ShareSubscribed {
        share_id: Uuid,
        entity_type: String,
        entity_id: Uuid,
    },
    /// Application-specific responses
    /// The Value should be an object with a "type" field for routing
    Application(Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Broadcast {
    /// A share was granted to this connection's user while connected; the
    /// connection already receives push events for it — no reconnect needed.
    ShareGranted {
        share_id: Uuid,
        entity_type: String,
        entity_id: Uuid,
    },
    /// A share held by this connection was revoked; the client should drop
    /// any locally held data for it.
    ShareRevoked { share_id: Uuid },
    /// Application-specific broadcasts
    /// The Value should be an object with a "type" field for routing
    Application(Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Message {
    Request { request: Request, id: String },
    Response { response: Response, id: String },
    Broadcast { broadcast: Broadcast },
    Error { message: String },
}
