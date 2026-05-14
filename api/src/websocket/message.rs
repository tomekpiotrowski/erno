use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    Version,
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
    /// Application-specific responses
    /// The Value should be an object with a "type" field for routing
    Application(Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Broadcast {
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
