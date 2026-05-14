use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use sea_orm::sea_query::Expr;
use serde::Deserialize;
use validator::Validate;

use crate::{
    api::validated_json::ValidatedJson,
    app::App,
    auth::handlers::issue_token_pair,
    database::models::{user, user_token, user_token_type::UserTokenType},
    jobs::send_password_reset_email_job::{SendPasswordResetEmailArgs, SendPasswordResetEmailJob},
    password::hash_password,
    token::{generate_secure_token, hash_token},
};

#[derive(Debug, Deserialize)]
pub struct PasswordResetRequestBody {
    pub email: String,
}

pub async fn password_reset_request<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    Json(body): Json<PasswordResetRequestBody>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let ok = (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "If that address is registered, a reset link is on its way"
        })),
    )
        .into_response();

    let user = user::Entity::find()
        .filter(user::Column::Email.eq(&body.email))
        .one(&app.db)
        .await;

    let user = match user {
        Ok(Some(u)) => u,
        _ => return ok,
    };

    let raw_token = generate_secure_token(64);
    let args = SendPasswordResetEmailArgs {
        user_id: user.id,
        email: user.email,
        raw_token,
    };
    let _ = app
        .run_job::<SendPasswordResetEmailJob<ExtraConfig>>(args)
        .await;

    ok
}

#[derive(Debug, Deserialize, Validate)]
pub struct PasswordResetConfirmBody {
    pub token: String,
    #[validate(length(min = 8))]
    pub new_password: String,
}

pub async fn password_reset_confirm<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    ValidatedJson(body): ValidatedJson<PasswordResetConfirmBody>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let token_hash = hash_token(&body.token);
    let now = Utc::now().naive_utc();

    let token_row = user_token::Entity::find()
        .filter(user_token::Column::TokenHash.eq(&token_hash))
        .filter(user_token::Column::TokenType.eq(UserTokenType::PasswordReset))
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

    // Atomically claim all password-reset tokens for this user. If rows_affected == 0
    // a concurrent request already consumed them — treat as expired/invalid.
    let delete_result = user_token::Entity::delete_many()
        .filter(user_token::Column::UserId.eq(user_id))
        .filter(user_token::Column::TokenType.eq(UserTokenType::PasswordReset))
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

    let new_hash = match hash_password(&body.new_password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let user_update = user::ActiveModel {
        id: Set(user_id),
        password_hash: Set(new_hash),
        ..Default::default()
    };
    if user_update.update(&app.db).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Invalidate all outstanding sessions by bumping the token version.
    if user::Entity::update_many()
        .col_expr(
            user::Column::TokenVersion,
            Expr::col(user::Column::TokenVersion).add(1),
        )
        .filter(user::Column::Id.eq(user_id))
        .exec(&app.db)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Re-fetch the user to get the updated token_version for the new JWT.
    let updated_user = match user::Entity::find_by_id(user_id).one(&app.db).await {
        Ok(Some(u)) => u,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    match issue_token_pair(&app, &updated_user).await {
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
    async fn test_password_reset_request_always_returns_200() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let response = t
            .server
            .post("/api/auth/password-reset/request")
            .json(&json!({ "email": "nobody@example.com" }))
            .await;
        assert_eq!(response.status_code(), 200);
    }

    #[tokio::test]
    async fn test_password_reset_request_enqueues_job_for_existing_user() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        user::ActiveModel {
            email: Set("reset_request@example.com".to_string()),
            password_hash: Set(hash_password("old_password").unwrap()),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let response = t
            .server
            .post("/api/auth/password-reset/request")
            .json(&json!({ "email": "reset_request@example.com" }))
            .await;
        assert_eq!(response.status_code(), 200);
        assert_eq!(
            t.enqueued_jobs_of_type("send_password_reset_email").len(),
            1
        );
    }

    #[tokio::test]
    async fn test_password_reset_confirm_with_valid_token_logs_in() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        let u = user::ActiveModel {
            email: Set("reset_confirm@example.com".to_string()),
            password_hash: Set(hash_password("old_password").unwrap()),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        user_token::ActiveModel {
            user_id: Set(u.id),
            token_type: Set(UserTokenType::PasswordReset),
            token_hash: Set(hash_token("valid_reset_token")),
            expires_at: Set((Utc::now() + chrono::Duration::hours(24)).naive_utc()),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let response = t
            .server
            .post("/api/auth/password-reset/confirm")
            .json(&json!({ "token": "valid_reset_token", "new_password": "new_password123" }))
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert!(body["token"].is_string());
        assert!(body["refresh_token"].is_string());
    }

    #[tokio::test]
    async fn test_password_reset_confirm_with_expired_token_returns_422() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let response = t
            .server
            .post("/api/auth/password-reset/confirm")
            .json(&json!({ "token": "bad_token", "new_password": "new_password123" }))
            .await;
        assert_eq!(response.status_code(), 422);
    }
}
