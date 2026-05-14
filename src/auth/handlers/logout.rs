use axum::{extract::State, http::StatusCode, response::IntoResponse};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use sea_orm::sea_query::Expr;

use crate::{app::App, auth::current_user::CurrentUser, database::models::user};

pub async fn logout<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
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
        auth::{jwt::generate_token, router::auth_router},
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

        // Logout should succeed
        let response = t
            .server
            .post("/api/auth/logout")
            .add_header("Authorization", format!("Bearer {token}"))
            .await;
        assert_eq!(response.status_code(), 204);

        // The same token should now be rejected
        let response = t
            .server
            .post("/api/auth/logout")
            .add_header("Authorization", format!("Bearer {token}"))
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

        // Do a login to get the current token (same version)
        let login_response = t
            .server
            .post("/api/auth/login")
            .json(&json!({ "email": "reset_invalidates@example.com", "password": "old_password" }))
            .await;
        assert_eq!(login_response.status_code(), 200);

        // Simulate a password reset by inserting a reset token then confirming it
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

        // The old token should now be rejected
        let response = t
            .server
            .post("/api/auth/logout")
            .add_header("Authorization", format!("Bearer {old_token}"))
            .await;
        assert_eq!(response.status_code(), 401);
    }
}
