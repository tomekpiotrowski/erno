use axum::{
    routing::{get, post},
    Router,
};

use crate::{
    app::App,
    dev::handlers::{
        clear_emails, clear_jobs, delete_email, email_body, email_preview, list_emails, list_jobs,
        retry_job,
    },
};

pub fn dev_router<ExtraConfig: Clone + Send + Sync + 'static>(app: App<ExtraConfig>) -> Router {
    Router::new()
        .route(
            "/dev/emails",
            get(list_emails::<ExtraConfig>).delete(clear_emails::<ExtraConfig>),
        )
        .route(
            "/dev/emails/{id}",
            axum::routing::delete(delete_email::<ExtraConfig>),
        )
        .route(
            "/dev/emails/{id}/preview",
            get(email_preview::<ExtraConfig>),
        )
        .route("/dev/emails/{id}/body", get(email_body::<ExtraConfig>))
        .route(
            "/dev/jobs",
            get(list_jobs::<ExtraConfig>).delete(clear_jobs::<ExtraConfig>),
        )
        .route("/dev/jobs/{id}/retry", post(retry_job::<ExtraConfig>))
        .with_state(app)
}
