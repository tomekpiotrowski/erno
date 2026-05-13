use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use sea_orm::{ActiveModelTrait, Set};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    api::validated_json::ValidatedJson,
    app::App,
    database::models::user,
    jobs::send_verification_email_job::{SendVerificationEmailArgs, SendVerificationEmailJob},
    password::hash_password,
    token::generate_secure_token,
};

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8))]
    pub password: String,
}

#[derive(Serialize)]
struct RegisterResponse {
    message: &'static str,
}

pub async fn register<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    ValidatedJson(body): ValidatedJson<RegisterRequest>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let password_hash = match hash_password(&body.password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let new_user = user::ActiveModel {
        email: Set(body.email.clone()),
        password_hash: Set(password_hash),
        ..Default::default()
    };

    let created_user = match new_user.insert(&app.db).await {
        Ok(u) => u,
        Err(e) => {
            if crate::api::unique_constraint::is_unique_violation(&e) {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({ "error": "email_taken" })),
                )
                    .into_response();
            }
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let raw_token = generate_secure_token(64);
    let args = SendVerificationEmailArgs {
        user_id: created_user.id,
        email: created_user.email.clone(),
        raw_token,
    };

    if app
        .run_job::<SendVerificationEmailJob<ExtraConfig>>(args)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    (
        StatusCode::CREATED,
        Json(RegisterResponse {
            message: "Check your email to verify your account",
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use serde_json::json;

    use crate::{
        app::App,
        auth::router::auth_router,
        database::migrations::Migrator,
        tests::setup_test::setup_test,
    };

    fn test_router(app: App) -> Router {
        Router::new().merge(auth_router(app))
    }
    fn no_fixtures(
        db: &sea_orm::DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let _ = db;
        })
    }

    #[tokio::test]
    async fn test_register_creates_user_and_enqueues_email() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        let response = t
            .server
            .post("/api/auth/register")
            .json(&json!({ "email": "user@example.com", "password": "password123" }))
            .await;

        assert_eq!(response.status_code(), 201);
        assert_eq!(t.enqueued_jobs_of_type("send_verification_email").len(), 1);
    }

    #[tokio::test]
    async fn test_register_duplicate_email_returns_409() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        t.server
            .post("/api/auth/register")
            .json(&json!({ "email": "dup@example.com", "password": "password123" }))
            .await;

        let response = t
            .server
            .post("/api/auth/register")
            .json(&json!({ "email": "dup@example.com", "password": "password123" }))
            .await;

        assert_eq!(response.status_code(), 409);
    }
}
