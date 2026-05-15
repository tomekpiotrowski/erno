use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use sea_orm::{EntityTrait, QueryOrder, QuerySelect};
use uuid::Uuid;

use crate::{app::App, database::models::job};

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

pub async fn list_jobs<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    match job::Entity::find()
        .order_by_desc(job::Column::CreatedAt)
        .limit(100)
        .all(&app.db)
        .await
    {
        Ok(jobs) => Json(jobs).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn clear_jobs<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    match job::Entity::delete_many().exec(&app.db).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
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
