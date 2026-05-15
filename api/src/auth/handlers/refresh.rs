use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;

use crate::{
    app::App,
    auth::handlers::issue_token_pair,
    database::models::{user, user_token, user_token_type::UserTokenType},
    token::hash_token,
};

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

pub async fn refresh<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    Json(body): Json<RefreshRequest>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let token_hash = hash_token(&body.refresh_token);
    let now = Utc::now().naive_utc();

    let token_row = user_token::Entity::find()
        .filter(user_token::Column::TokenHash.eq(&token_hash))
        .filter(user_token::Column::TokenType.eq(UserTokenType::RefreshToken))
        .filter(user_token::Column::ExpiresAt.gt(now))
        .one(&app.db)
        .await;

    let token_row = match token_row {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid_or_expired_token" })),
            )
                .into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let user_id = token_row.user_id;
    let token_id = token_row.id;

    // Atomically consume the refresh token. rows_affected == 0 means a concurrent
    // request already rotated it — reject to prevent replay.
    let delete_result = user_token::Entity::delete_by_id(token_id)
        .exec(&app.db)
        .await;

    match delete_result {
        Ok(r) if r.rows_affected == 0 => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid_or_expired_token" })),
            )
                .into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Ok(_) => {}
    }

    let user = match user::Entity::find_by_id(user_id).one(&app.db).await {
        Ok(Some(u)) => u,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    match issue_token_pair(&app, &user).await {
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
        database::{
            migrations::Migrator,
            models::{user, user_token, user_token_type::UserTokenType},
        },
        password::hash_password,
        tests::setup_test::setup_test,
        token::hash_token,
    };

    fn test_router(_app: App) -> Router {
        Router::new()
    }
    fn no_fixtures(
        db: &sea_orm::DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let _ = db;
        })
    }

    #[tokio::test]
    async fn test_refresh_returns_new_token_pair() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        let u = user::ActiveModel {
            email: Set("refresh@example.com".to_string()),
            password_hash: Set(hash_password("password123").unwrap()),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        user_token::ActiveModel {
            user_id: Set(u.id),
            token_type: Set(UserTokenType::RefreshToken),
            token_hash: Set(hash_token("valid_refresh_token")),
            expires_at: Set((Utc::now() + chrono::Duration::days(30)).naive_utc()),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let response = t
            .server
            .post("/api/auth/refresh")
            .json(&json!({ "refresh_token": "valid_refresh_token" }))
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert!(body["access_token"].is_string());
        assert!(body["refresh_token"].is_string());
        assert_ne!(body["refresh_token"].as_str().unwrap(), "valid_refresh_token");
    }

    #[tokio::test]
    async fn test_refresh_token_rotation_prevents_replay() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        let u = user::ActiveModel {
            email: Set("refresh_replay@example.com".to_string()),
            password_hash: Set(hash_password("password123").unwrap()),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        user_token::ActiveModel {
            user_id: Set(u.id),
            token_type: Set(UserTokenType::RefreshToken),
            token_hash: Set(hash_token("rotate_me")),
            expires_at: Set((Utc::now() + chrono::Duration::days(30)).naive_utc()),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let first = t
            .server
            .post("/api/auth/refresh")
            .json(&json!({ "refresh_token": "rotate_me" }))
            .await;
        assert_eq!(first.status_code(), 200);

        // Replaying the same token must be rejected.
        let second = t
            .server
            .post("/api/auth/refresh")
            .json(&json!({ "refresh_token": "rotate_me" }))
            .await;
        assert_eq!(second.status_code(), 401);
    }

    #[tokio::test]
    async fn test_refresh_with_invalid_token_returns_401() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let response = t
            .server
            .post("/api/auth/refresh")
            .json(&json!({ "refresh_token": "nonexistent" }))
            .await;
        assert_eq!(response.status_code(), 401);
    }
}
