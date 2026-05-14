use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use crate::app::App;

pub async fn list_emails<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    let records = app.mailer.records().unwrap_or_default();
    Json(records)
}

pub async fn clear_emails<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    app.mailer.clear_messages();
    StatusCode::NO_CONTENT
}

pub async fn delete_email<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if app.mailer.remove_record(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}
