use axum::{
    routing::{delete, post},
    Router,
};

use crate::{
    app::App,
    share::handlers::{
        create::create,
        grant::grant,
        list::list,
        revoke::{revoke, revoke_grant},
    },
};

/// Mount all share routes.
///
/// Usage in your app router:
/// ```rust,ignore
/// app_router.nest("/shares", share_router(app.clone()));
/// ```
///
/// Routes (all require JWT; share *consumption* happens via the
/// `X-Erno-Share` header on shared endpoints, not here):
/// - `POST   /`                        — create a share (link and/or direct grants)
/// - `GET    /`                        — list own shares with grants
/// - `POST   /{id}/grants`             — add a recipient (active immediately)
/// - `DELETE /{id}`                    — revoke the whole share
/// - `DELETE /{id}/grants/{user_id}`   — revoke a single grant
pub fn share_router<ExtraConfig: Clone + Send + Sync + 'static>(app: App<ExtraConfig>) -> Router {
    Router::new()
        .route("/", post(create::<ExtraConfig>).get(list::<ExtraConfig>))
        .route("/{id}", delete(revoke::<ExtraConfig>))
        .route("/{id}/grants", post(grant::<ExtraConfig>))
        .route(
            "/{id}/grants/{user_id}",
            delete(revoke_grant::<ExtraConfig>),
        )
        .with_state(app)
}
