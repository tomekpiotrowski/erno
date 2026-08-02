//! Docs: docs/src/content/docs/api/console.md
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    admin::{
        auth::AdminAuth,
        dto::{ErrorBody, GiftRequest, PlansResponse},
        service,
    },
    app::App,
    database::models::job_status::JobStatus,
};

fn db_error(e: sea_orm::DbErr) -> axum::response::Response {
    tracing::error!("Admin API database error: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            error: "internal_error".to_string(),
        }),
    )
        .into_response()
}

pub async fn get_dashboard<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match service::dashboard(&app.db).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct UsersQuery {
    pub q: Option<String>,
}

pub async fn list_users<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
    Query(query): Query<UsersQuery>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match service::list_users(&app.db, query.q.as_deref()).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

pub async fn get_user<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
    Path(user_id): Path<Uuid>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match service::user_detail(&app.db, user_id).await {
        Ok(Some(body)) => Json(body).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "user_not_found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => db_error(e),
    }
}

pub async fn activate_user<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
    Path(user_id): Path<Uuid>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match service::activate_user(&app.db, user_id).await {
        Ok(Some(body)) => {
            tracing::info!(%user_id, "Admin activated user");
            Json(body).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "user_not_found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => db_error(e),
    }
}

pub async fn delete_user<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
    Path(user_id): Path<Uuid>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match service::delete_user(
        &app.db,
        &app.job_queue,
        app.user_data_deleter.as_ref(),
        user_id,
    )
    .await
    {
        Ok(true) => {
            tracing::info!(%user_id, "Admin deleted user");
            app.websocket_connections.disconnect_user(user_id).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "user_not_found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => db_error(e),
    }
}

pub async fn gift_user<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
    Path(user_id): Path<Uuid>,
    Json(body): Json<GiftRequest>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    if body.plan.is_empty() || body.duration_days == 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "invalid_gift_request".to_string(),
            }),
        )
            .into_response();
    }

    match service::gift_subscription(&app.db, user_id, body.plan, body.duration_days).await {
        Ok(Some(detail)) => {
            tracing::info!(%user_id, "Admin gifted subscription");
            (StatusCode::CREATED, Json(detail)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "user_not_found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => db_error(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct JobsQuery {
    pub status: Option<JobStatus>,
    #[serde(rename = "type")]
    pub job_type: Option<String>,
}

pub async fn list_jobs<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
    Query(query): Query<JobsQuery>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match service::list_jobs(&app.db, query.status, query.job_type.as_deref()).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}

pub async fn retry_job<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
    Path(job_id): Path<Uuid>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    match service::retry_job(&app.db, job_id).await {
        Ok(true) => {
            tracing::info!(%job_id, "Admin retried job");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "job_not_found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => db_error(e),
    }
}

pub async fn list_plans<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let plans: Vec<String> = app
        .config
        .stripe
        .as_ref()
        .map(|s| s.price_ids.keys().cloned().collect())
        .unwrap_or_default();
    Json(PlansResponse { plans })
}

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    /// Window in days (default 7; allowed 1–365).
    pub days: Option<i64>,
}

pub async fn get_stats<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    _auth: AdminAuth,
    Query(query): Query<StatsQuery>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let days = query.days.unwrap_or(7);
    match service::business_stats(&app.db, days).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => db_error(e),
    }
}
