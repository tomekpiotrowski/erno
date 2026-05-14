use axum::{routing::post, Router};

use crate::app::App;

use super::handlers::{
    login::login,
    logout::logout,
    password_reset::{password_reset_confirm, password_reset_request},
    refresh::refresh,
    register::register,
    resend_verification::resend_verification,
    verify_email::verify_email,
};

/// Returns a router with all auth endpoints mounted under `/auth/`.
///
/// Mount this in your `app_router` function:
/// ```rust,ignore
/// fn app_router(app: App) -> Router {
///     Router::new()
///         .merge(erno::auth::auth_router(app))
///         .route("/posts", get(list_posts))
/// }
/// ```
pub fn auth_router<ExtraConfig>(app: App<ExtraConfig>) -> Router
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/register", post(register::<ExtraConfig>))
        .route("/auth/login", post(login::<ExtraConfig>))
        .route("/auth/logout", post(logout::<ExtraConfig>))
        .route("/auth/email/verify", post(verify_email::<ExtraConfig>))
        .route(
            "/auth/email/resend-verification",
            post(resend_verification::<ExtraConfig>),
        )
        .route(
            "/auth/password-reset/request",
            post(password_reset_request::<ExtraConfig>),
        )
        .route(
            "/auth/password-reset/confirm",
            post(password_reset_confirm::<ExtraConfig>),
        )
        .route("/auth/refresh", post(refresh::<ExtraConfig>))
        .with_state(app)
}
