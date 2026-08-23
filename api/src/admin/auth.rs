//! Docs: docs/src/content/docs/api/console.md
use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

use crate::{admin::dto::ErrorBody, app::App, config::AdminConfig, password::verify_password};

/// Marker extracted when Basic auth succeeds against configured admin credentials.
pub struct AdminAuth;

pub enum AdminAuthError {
    Unauthorized,
    Misconfigured,
}

impl IntoResponse for AdminAuthError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized => {
                let mut res = (
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorBody {
                        error: "unauthorized".to_string(),
                    }),
                )
                    .into_response();
                res.headers_mut().insert(
                    header::WWW_AUTHENTICATE,
                    HeaderValue::from_static("Basic realm=\"Erno Admin\""),
                );
                res
            }
            Self::Misconfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorBody {
                    error: "admin_not_configured".to_string(),
                }),
            )
                .into_response(),
        }
    }
}

/// Verify operator Basic auth against the configured admin credentials.
///
/// Factored out of the extractor so other routers can enforce the same check
/// without a second argon2 implementation — notably the monitoring collector,
/// which carries a composite state and so cannot use [`AdminAuth`] directly.
///
/// # Errors
///
/// [`AdminAuthError::Misconfigured`] when no admin password is configured, and
/// [`AdminAuthError::Unauthorized`] when the header is absent, malformed, or
/// the credentials do not match.
pub fn verify_admin_basic_auth(
    admin: Option<&AdminConfig>,
    headers: &HeaderMap,
) -> Result<(), AdminAuthError> {
    let admin = admin
        .filter(|c| c.is_enabled())
        .ok_or(AdminAuthError::Misconfigured)?;

    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(AdminAuthError::Unauthorized)?;

    let encoded = header
        .strip_prefix("Basic ")
        .or_else(|| header.strip_prefix("basic "))
        .ok_or(AdminAuthError::Unauthorized)?;

    let decoded = BASE64
        .decode(encoded.trim())
        .map_err(|_| AdminAuthError::Unauthorized)?;
    let decoded = String::from_utf8(decoded).map_err(|_| AdminAuthError::Unauthorized)?;

    let (username, password) = decoded
        .split_once(':')
        .ok_or(AdminAuthError::Unauthorized)?;

    if username != admin.username {
        return Err(AdminAuthError::Unauthorized);
    }

    match verify_password(password, &admin.password_hash) {
        Ok(true) => Ok(()),
        Ok(false) => Err(AdminAuthError::Unauthorized),
        Err(e) => {
            tracing::error!("Admin auth: password verification error: {e}");
            Err(AdminAuthError::Unauthorized)
        }
    }
}

impl<ExtraConfig> FromRequestParts<App<ExtraConfig>> for AdminAuth
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    type Rejection = AdminAuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &App<ExtraConfig>,
    ) -> Result<Self, Self::Rejection> {
        verify_admin_basic_auth(state.config.admin.as_ref(), &parts.headers).map(|()| AdminAuth)
    }
}
