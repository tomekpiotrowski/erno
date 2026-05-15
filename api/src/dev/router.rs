use axum::{routing::get, Router};

use crate::{
    app::App,
    dev::handlers::{clear_emails, clear_jobs, delete_email, list_emails, list_jobs},
};

pub fn dev_router<ExtraConfig: Clone + Send + Sync + 'static>(app: App<ExtraConfig>) -> Router {
    Router::new()
        .route(
            "/dev/emails",
            get(list_emails::<ExtraConfig>).delete(clear_emails::<ExtraConfig>),
        )
        .route("/dev/emails/{id}", axum::routing::delete(delete_email::<ExtraConfig>))
        .route(
            "/dev/jobs",
            get(list_jobs::<ExtraConfig>).delete(clear_jobs::<ExtraConfig>),
        )
        .with_state(app)
}
