use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;

use crate::{
    app::App,
    auth::current_user::CurrentUser,
    database::models::{user, user_token, user_token_type::UserTokenType},
    token::hash_token,
};

#[derive(Debug, Deserialize, Default)]
pub struct LogoutRequest {
    pub refresh_token: Option<String>,
}

pub async fn logout<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
    body: Option<Json<LogoutRequest>>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let refresh_token = body.and_then(|b| b.0.refresh_token);

    if let Some(raw_token) = refresh_token {
        let token_hash = hash_token(&raw_token);
        let _ = user_token::Entity::delete_many()
            .filter(user_token::Column::UserId.eq(current_user.id))
            .filter(user_token::Column::TokenType.eq(UserTokenType::RefreshToken))
            .filter(user_token::Column::TokenHash.eq(token_hash))
            .exec(&app.db)
            .await;
    }

    let result = user::Entity::update_many()
        .col_expr(
            user::Column::TokenVersion,
            Expr::col(user::Column::TokenVersion).add(1),
        )
        .filter(user::Column::Id.eq(current_user.id))
        .exec(&app.db)
        .await;

    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
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
        auth::jwt::generate_token,
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
    async fn test_logout_invalidates_token() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        let u = user::ActiveModel {
            email: Set("logout@example.com".to_string()),
            password_hash: Set(hash_password("password123").unwrap()),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let token = generate_token(&t.config, u.id, u.token_version).unwrap();

        let response = t
            .server
            .post("/api/auth/logout")
            .add_header("Authorization", format!("Bearer {token}"))
            .await;
        assert_eq!(response.status_code(), 204);

        let response = t
            .server
            .post("/api/auth/logout")
            .add_header("Authorization", format!("Bearer {token}"))
            .await;
        assert_eq!(response.status_code(), 401);
    }

    #[tokio::test]
    async fn test_logout_deletes_refresh_token() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        let u = user::ActiveModel {
            email: Set("logout_refresh@example.com".to_string()),
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
            token_hash: Set(hash_token("my_refresh_token")),
            expires_at: Set((Utc::now() + chrono::Duration::days(30)).naive_utc()),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let token = generate_token(&t.config, u.id, u.token_version).unwrap();
        let response = t
            .server
            .post("/api/auth/logout")
            .add_header("Authorization", format!("Bearer {token}"))
            .json(&json!({ "refresh_token": "my_refresh_token" }))
            .await;
        assert_eq!(response.status_code(), 204);

        // Refresh token should now be gone
        let response = t
            .server
            .post("/api/auth/refresh")
            .json(&json!({ "refresh_token": "my_refresh_token" }))
            .await;
        assert_eq!(response.status_code(), 401);
    }

    #[tokio::test]
    async fn test_logout_requires_auth() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let response = t.server.post("/api/auth/logout").await;
        assert_eq!(response.status_code(), 401);
    }

    #[tokio::test]
    async fn test_password_reset_invalidates_previous_token() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        let u = user::ActiveModel {
            email: Set("reset_invalidates@example.com".to_string()),
            password_hash: Set(hash_password("old_password").unwrap()),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let old_token = generate_token(&t.config, u.id, u.token_version).unwrap();

        let login_response = t
            .server
            .post("/api/auth/login")
            .json(&json!({ "email": "reset_invalidates@example.com", "password": "old_password" }))
            .await;
        assert_eq!(login_response.status_code(), 200);

        use crate::{
            database::models::{user_token, user_token_type::UserTokenType},
            token::hash_token,
        };
        user_token::ActiveModel {
            user_id: Set(u.id),
            token_type: Set(UserTokenType::PasswordReset),
            token_hash: Set(hash_token("reset_token_abc")),
            expires_at: Set((Utc::now() + chrono::Duration::hours(1)).naive_utc()),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let reset_response = t
            .server
            .post("/api/auth/password-reset/confirm")
            .json(&json!({ "token": "reset_token_abc", "new_password": "new_password123" }))
            .await;
        assert_eq!(reset_response.status_code(), 200);

        let response = t
            .server
            .post("/api/auth/logout")
            .add_header("Authorization", format!("Bearer {old_token}"))
            .await;
        assert_eq!(response.status_code(), 401);
    }
}
