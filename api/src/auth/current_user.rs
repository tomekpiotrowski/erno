use async_trait::async_trait;
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use sea_orm::{DatabaseConnection, EntityTrait, Set};
use uuid::Uuid;

use crate::app::App;
use crate::auth::jwt;
use crate::database::models::user;

/// Trait for loading app-specific profile data alongside the authenticated user.
///
/// Implement this on your profile entity to enable `CurrentUser<YourProfile>`.
/// The `()` implementation is a no-op used when profile data is not needed.
#[async_trait]
pub trait LoadForUser: Sized + Send + Sync + 'static {
    async fn load_for_user(user_id: Uuid, db: &DatabaseConnection) -> Result<Self, AuthError>;
}

#[async_trait]
impl LoadForUser for () {
    async fn load_for_user(_: Uuid, _: &DatabaseConnection) -> Result<(), AuthError> {
        Ok(())
    }
}

/// Authenticated user extracted from the JWT Bearer token.
///
/// `P` is an optional app-defined profile type loaded from the database
/// alongside the user. Use `CurrentUser` (no type param) when you only
/// need the base user. Use `CurrentUser<YourProfile>` to load both.
///
/// # Example
///
/// ```rust,ignore
/// // Base user only
/// async fn list_posts(CurrentUser { user, .. }: CurrentUser) { ... }
///
/// // With app profile
/// async fn update_profile(CurrentUser { user, profile }: CurrentUser<Profile>) { ... }
/// ```
#[derive(Debug, Clone)]
pub struct CurrentUser<P: LoadForUser = ()> {
    pub user: user::Model,
    pub profile: P,
}

impl<P: LoadForUser> std::ops::Deref for CurrentUser<P> {
    type Target = user::Model;

    fn deref(&self) -> &Self::Target {
        &self.user
    }
}

/// Error type for authentication failures.
#[derive(Debug)]
pub enum AuthError {
    Unauthorized,
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

impl<ExtraConfig, P> FromRequestParts<App<ExtraConfig>> for CurrentUser<P>
where
    ExtraConfig: Clone + Send + Sync + 'static,
    P: LoadForUser,
{
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &App<ExtraConfig>,
    ) -> Result<Self, AuthError> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or(AuthError::Unauthorized)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::Unauthorized)?;

        let claims =
            jwt::verify_token(&state.config, token).map_err(|_| AuthError::Unauthorized)?;

        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AuthError::Unauthorized)?;

        let user = user::Entity::find_by_id(user_id)
            .one(&state.db)
            .await
            .map_err(|_| AuthError::DatabaseError)?
            .ok_or(AuthError::Unauthorized)?;

        // Reject tokens whose version no longer matches the stored version.
        // token_version is incremented on logout and password change.
        if claims.ver != user.token_version {
            return Err(AuthError::Unauthorized);
        }

        touch_last_active(&state.db, &user).await;

        let profile = P::load_for_user(user_id, &state.db).await?;

        Ok(CurrentUser { user, profile })
    }
}

/// How stale `last_active_at` must be before a request bothers rewriting it —
/// keeps this to roughly one write per user per window instead of one per request.
fn last_active_throttle() -> Duration {
    Duration::minutes(15)
}

/// Best-effort activity timestamp update; a failure here must never fail
/// an otherwise-valid authenticated request.
async fn touch_last_active(db: &DatabaseConnection, user: &user::Model) {
    let now = Utc::now().naive_utc();
    let stale = user
        .last_active_at
        .is_none_or(|t| now - t > last_active_throttle());
    if !stale {
        return;
    }

    let _ = user::Entity::update(user::ActiveModel {
        id: Set(user.id),
        last_active_at: Set(Some(now)),
        ..Default::default()
    })
    .exec(db)
    .await;
}
