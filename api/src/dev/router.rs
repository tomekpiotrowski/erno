use axum::{routing::get, Router};

use crate::{
    app::App,
    dev::handlers::{clear_emails, delete_email, list_emails},
};

pub fn dev_router<ExtraConfig: Clone + Send + Sync + 'static>(app: App<ExtraConfig>) -> Router {
    Router::new()
        .route(
            "/dev/emails",
            get(list_emails::<ExtraConfig>).delete(clear_emails::<ExtraConfig>),
        )
        .route("/dev/emails/{id}", axum::routing::delete(delete_email::<ExtraConfig>))
        .with_state(app)
}
