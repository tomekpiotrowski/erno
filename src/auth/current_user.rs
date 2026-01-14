use axum::{
    extract::{FromRef, FromRequestParts},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use sea_orm::{DatabaseConnection, EntityTrait, ModelTrait, PrimaryKeyTrait};
use uuid::Uuid;

use crate::auth::jwt;
use crate::config::Config;

/// Authenticated user extracted from JWT token.
///
/// This extractor loads the full user model from the database based on the JWT token
/// in the Authorization header. Use this in handlers that require authentication.
///
/// The generic parameter U should be your user model type (must have UUID primary key).
///
/// # Example
/// ```rust,ignore
/// use api_core::auth::CurrentUser;
/// use crate::database::models::user;
///
/// pub async fn my_handler(current_user: CurrentUser<user::Model>) -> RequestResult {
///     // Access the authenticated user via Deref
///     println!("User email: {}", current_user.email);
///     Ok(RequestSuccess::Ok)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct CurrentUser<U> {
    /// The loaded user model from the database
    pub user: U,
}

impl<U> std::ops::Deref for CurrentUser<U> {
    type Target = U;

    fn deref(&self) -> &Self::Target {
        &self.user
    }
}

/// Error type for CurrentUser extraction failures.
#[derive(Debug)]
pub enum AuthError {
    /// No Authorization header provided or invalid format
    Unauthorized,
    /// Database error while loading user
    DatabaseError,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            AuthError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
            AuthError::DatabaseError => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
            }
        }
    }
}

impl<S, U, E> FromRequestParts<S> for CurrentUser<U>
where
    S: Send + Sync,
    U: ModelTrait<Entity = E> + Send + sea_orm::FromQueryResult,
    E: EntityTrait<Model = U> + Send,
    <E::PrimaryKey as PrimaryKeyTrait>::ValueType: From<Uuid>,
    Config: FromRef<S>,
    DatabaseConnection: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts<'life0, 'life1>(
        parts: &'life0 mut Parts,
        state: &'life1 S,
    ) -> Result<Self, Self::Rejection> {
        // Extract Authorization header
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or(AuthError::Unauthorized)?;

        // Extract token (format: "Bearer <token>")
        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::Unauthorized)?;

        // Verify JWT and extract claims
        let config = Config::from_ref(state);
        let claims = jwt::verify_token(&config, token).map_err(|_| AuthError::Unauthorized)?;

        // Parse user ID from claims
        let user_id =
            Uuid::parse_str(&claims.sub).map_err(|_| AuthError::Unauthorized)?;

        // Load user from database
        let db = DatabaseConnection::from_ref(state);
        let user = E::find_by_id(user_id)
            .one(&db)
            .await
            .map_err(|_| AuthError::DatabaseError)?
            .ok_or(AuthError::Unauthorized)?;

        Ok(CurrentUser { user })
    }
}
