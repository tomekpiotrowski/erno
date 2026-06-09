use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};

use crate::app::App;
use crate::auth::current_user::{AuthError, CurrentUser};
use crate::share::principal::{resolve_principal, Principal};

/// HTTP header carrying raw share link tokens (repeatable / comma-separated).
///
/// The token travels here — never in a query parameter — so it stays out of
/// access logs, `Referer` headers, and browser history. Links carry it in the
/// URL fragment, which the client lifts into this header.
pub const SHARE_TOKEN_HEADER: &str = "x-erno-share";

/// Extracts a [`Principal`] from the request.
///
/// - The user is taken from the `Authorization` JWT if present. A missing
///   header yields an anonymous principal; an *invalid* token is still
///   rejected with 401 rather than silently downgraded.
/// - Link share tokens are read from the `X-Erno-Share` header.
/// - For authenticated users, active account grants are loaded as well.
impl<ExtraConfig> FromRequestParts<App<ExtraConfig>> for Principal
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &App<ExtraConfig>,
    ) -> Result<Self, AuthError> {
        let user = if parts.headers.contains_key(AUTHORIZATION) {
            let current_user = CurrentUser::<()>::from_request_parts(parts, state).await?;
            Some(current_user.user)
        } else {
            None
        };

        let raw_tokens: Vec<String> = parts
            .headers
            .get_all(SHARE_TOKEN_HEADER)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(String::from)
            .collect();

        resolve_principal(&state.db, user, &raw_tokens)
            .await
            .map_err(|_| AuthError::DatabaseError)
    }
}
