use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;

use crate::{
    app::App,
    auth::handlers::issue_token_pair,
    database::models::{user, user_token, user_token_type::UserTokenType},
    token::hash_token,
};

#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}

pub async fn verify_email<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    Json(body): Json<VerifyEmailRequest>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let token_hash = hash_token(&body.token);
    let now = Utc::now().naive_utc();

    let token_row = user_token::Entity::find()
        .filter(user_token::Column::TokenHash.eq(&token_hash))
        .filter(user_token::Column::TokenType.eq(UserTokenType::EmailVerification))
        .filter(user_token::Column::ExpiresAt.gt(now))
        .one(&app.db)
        .await;

    let token_row = match token_row {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": "invalid_or_expired_token" })),
            )
                .into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let user_id = token_row.user_id;

    // Atomically claim all email-verification tokens for this user. If rows_affected == 0
    // a concurrent request already consumed them — treat as expired/invalid.
    let delete_result = user_token::Entity::delete_many()
        .filter(user_token::Column::UserId.eq(user_id))
        .filter(user_token::Column::TokenType.eq(UserTokenType::EmailVerification))
        .exec(&app.db)
        .await;

    match delete_result {
        Ok(r) if r.rows_affected == 0 => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": "invalid_or_expired_token" })),
            )
                .into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Ok(_) => {}
    }

    let user_update = user::ActiveModel {
        id: Set(user_id),
        email_verified_at: Set(Some(now)),
        ..Default::default()
    };
    if user_update.update(&app.db).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Re-fetch the user to get the current token_version for the JWT.
    let verified_user = match user::Entity::find_by_id(user_id).one(&app.db).await {
        Ok(Some(u)) => u,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    match issue_token_pair(&app, &verified_user).await {
        Ok(pair) => (StatusCode::OK, Json(pair)).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, Set};
    use serde_json::json;

    use crate::{
        app::App,
        auth::router::auth_router,
        database::{
            migrations::Migrator,
            models::{user, user_token, user_token_type::UserTokenType},
        },
        password::hash_password,
        tests::setup_test::setup_test,
        token::hash_token,
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
    async fn test_verify_email_with_valid_token_returns_jwt() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        let u = user::ActiveModel {
            email: Set("verify_valid@example.com".to_string()),
            password_hash: Set(hash_password("password123").unwrap()),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        user_token::ActiveModel {
            user_id: Set(u.id),
            token_type: Set(UserTokenType::EmailVerification),
            token_hash: Set(hash_token("valid_token_for_verify")),
            expires_at: Set((Utc::now() + chrono::Duration::hours(24)).naive_utc()),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let response = t
            .server
            .post("/api/auth/email/verify")
            .json(&json!({ "token": "valid_token_for_verify" }))
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert!(body["token"].is_string());
        assert!(body["refresh_token"].is_string());
    }

    #[tokio::test]
    async fn test_verify_email_with_invalid_token_returns_422() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let response = t
            .server
            .post("/api/auth/email/verify")
            .json(&json!({ "token": "bad_token" }))
            .await;
        assert_eq!(response.status_code(), 422);
    }
}
