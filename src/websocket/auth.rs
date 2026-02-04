use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::app::App;
use crate::auth::jwt;

/// Query parameters for WebSocket authentication
#[derive(Debug, Deserialize)]
pub struct WsAuthQuery {
    /// JWT token for authentication
    pub token: Option<String>,
}

/// Extract JWT token from query parameter or Authorization header
fn extract_token(query: &WsAuthQuery, headers: &HeaderMap) -> Option<String> {
    // Try query parameter first (easier for browser WebSocket API)
    if let Some(token) = &query.token {
        return Some(token.clone());
    }

    // Try Authorization header as fallback
    if let Some(auth_header) = headers.get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }

    None
}

/// WebSocket handler with JWT authentication
///
/// Authenticates the user via JWT token (from query param or header),
/// then upgrades the connection and passes the user_id to the connection handler.
pub async fn authenticated_ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsAuthQuery>,
    headers: HeaderMap,
    State(app): State<App>,
) -> Response {
    // Extract token from query or header
    let Some(token) = extract_token(&query, &headers) else {
        return (StatusCode::UNAUTHORIZED, "Missing token").into_response();
    };

    // Verify JWT token
    let claims = match jwt::verify_token(&app.config, &token) {
        Ok(claims) => claims,
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, "Invalid token").into_response();
        }
    };

    // Parse user_id from claims
    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, "Invalid user ID in token").into_response();
        }
    };

    // Get connections from app state
    let connections = app.websocket_connections.clone();

    // Upgrade to WebSocket with the authenticated user_id
    ws.on_upgrade(move |socket| async move { connections.handle_socket(user_id, socket).await })
}
