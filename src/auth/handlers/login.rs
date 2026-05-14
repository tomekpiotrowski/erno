use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    api::validated_json::ValidatedJson,
    app::App,
    auth::jwt::generate_token,
    database::models::user,
    password::verify_password,
};

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(email)]
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
}

pub async fn login<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    ValidatedJson(body): ValidatedJson<LoginRequest>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let user = user::Entity::find()
        .filter(user::Column::Email.eq(&body.email))
        .one(&app.db)
        .await;

    let user = match user {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid_credentials" })),
            )
                .into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    match verify_password(&body.password, &user.password_hash) {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid_credentials" })),
            )
                .into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }

    if user.email_verified_at.is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "email_not_verified" })),
        )
            .into_response();
    }

    match generate_token(&app.config, user.id, user.token_version) {
        Ok(token) => (StatusCode::OK, Json(LoginResponse { token })).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use sea_orm::{ActiveModelTrait, Set};
    use serde_json::json;

    use crate::{
        app::App,
        auth::router::auth_router,
        database::{migrations::Migrator, models::user},
        password::hash_password,
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
    async fn test_login_with_valid_credentials_returns_token() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        user::ActiveModel {
            email: Set("verified@example.com".to_string()),
            password_hash: Set(hash_password("password123").unwrap()),
            email_verified_at: Set(Some(chrono::Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let response = t
            .server
            .post("/api/auth/login")
            .json(&json!({ "email": "verified@example.com", "password": "password123" }))
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert!(body["token"].is_string());
    }

    #[tokio::test]
    async fn test_login_with_wrong_password_returns_401() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        user::ActiveModel {
            email: Set("verified2@example.com".to_string()),
            password_hash: Set(hash_password("password123").unwrap()),
            email_verified_at: Set(Some(chrono::Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let response = t
            .server
            .post("/api/auth/login")
            .json(&json!({ "email": "verified2@example.com", "password": "wrong" }))
            .await;
        assert_eq!(response.status_code(), 401);
    }

    #[tokio::test]
    async fn test_login_unverified_user_returns_403() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        t.server
            .post("/api/auth/register")
            .json(&json!({ "email": "unverified@example.com", "password": "password123" }))
            .await;

        let response = t
            .server
            .post("/api/auth/login")
            .json(&json!({ "email": "unverified@example.com", "password": "password123" }))
            .await;
        assert_eq!(response.status_code(), 403);
    }
}
