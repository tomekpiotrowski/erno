use std::sync::Arc;

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use sea_orm::EntityTrait;
use serde::Deserialize;
use uuid::Uuid;

use crate::app::App;
use crate::auth::jwt;
use crate::database::models::user;
use crate::share::principal::{resolve_principal, resolve_share_token, Principal};
use crate::websocket::connections::ShareTokenValidator;

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

/// WebSocket handler with JWT or anonymous authentication.
///
/// With a JWT (query param or header) the connection's [`Principal`] is the
/// authenticated user plus their active share grants. Without one the
/// connection is anonymous with an empty principal — it receives nothing
/// until it attaches shares via the `subscribe-share` control message.
/// An *invalid* JWT is still rejected rather than silently downgraded.
///
/// Share link tokens are deliberately not accepted in the upgrade URL: they
/// ride the `subscribe-share` message post-connect, keeping them out of
/// access logs.
pub async fn authenticated_ws_handler<ExtraConfig>(
    ws: WebSocketUpgrade,
    Query(query): Query<WsAuthQuery>,
    headers: HeaderMap,
    State(app): State<App<ExtraConfig>>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let principal = match extract_token(&query, &headers) {
        Some(token) => {
            // Verify JWT token
            let claims = match jwt::verify_token(&app.config, &token) {
                Ok(claims) => claims,
                Err(_) => {
                    return (StatusCode::UNAUTHORIZED, "Invalid token").into_response();
                }
            };

            let user_id = match Uuid::parse_str(&claims.sub) {
                Ok(id) => id,
                Err(_) => {
                    return (StatusCode::UNAUTHORIZED, "Invalid user ID in token").into_response();
                }
            };

            let found_user = match user::Entity::find_by_id(user_id).one(&app.db).await {
                Ok(found) => found,
                Err(_) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
                        .into_response();
                }
            };
            let Some(found_user) = found_user else {
                return (StatusCode::UNAUTHORIZED, "Unknown user").into_response();
            };
            if claims.ver != found_user.token_version {
                return (StatusCode::UNAUTHORIZED, "Invalid token").into_response();
            }

            // Load the user's active share grants into the initial principal.
            match resolve_principal(&app.db, Some(found_user), &[]).await {
                Ok(principal) => principal,
                Err(_) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
                        .into_response();
                }
            }
        }
        None => Principal::default(),
    };

    let connections = app.websocket_connections.clone();

    // Validates raw share tokens arriving via subscribe-share messages.
    let db = app.db.clone();
    let share_validator: ShareTokenValidator = Arc::new(move |raw_token: String| {
        let db = db.clone();
        Box::pin(async move {
            resolve_share_token(&db, &raw_token)
                .await
                .ok()
                .flatten()
        })
    });

    ws.on_upgrade(move |socket| async move {
        connections
            .handle_socket(principal, socket, Some(share_validator))
            .await;
    })
}
