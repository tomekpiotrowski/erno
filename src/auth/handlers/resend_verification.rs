use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;

use crate::{
    app::App,
    database::models::user,
    jobs::send_verification_email_job::{SendVerificationEmailArgs, SendVerificationEmailJob},
    token::generate_secure_token,
};

#[derive(Debug, Deserialize)]
pub struct ResendVerificationRequest {
    pub email: String,
}

pub async fn resend_verification<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    Json(body): Json<ResendVerificationRequest>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let ok = (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "If that address is registered and unverified, a new email is on its way"
        })),
    )
        .into_response();

    let user = user::Entity::find()
        .filter(user::Column::Email.eq(&body.email))
        .one(&app.db)
        .await;

    let user = match user {
        Ok(Some(u)) if u.email_verified_at.is_none() => u,
        _ => return ok,
    };

    let raw_token = generate_secure_token(64);
    let args = SendVerificationEmailArgs {
        user_id: user.id,
        email: user.email,
        raw_token,
    };
    let _ = app
        .run_job::<SendVerificationEmailJob<ExtraConfig>>(args)
        .await;

    ok
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
    async fn test_resend_always_returns_200() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let response = t
            .server
            .post("/api/auth/email/resend-verification")
            .json(&json!({ "email": "nobody@example.com" }))
            .await;
        assert_eq!(response.status_code(), 200);
    }

    #[tokio::test]
    async fn test_resend_for_unverified_user_enqueues_job() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;

        user::ActiveModel {
            email: Set("resend_unverified@example.com".to_string()),
            password_hash: Set(hash_password("password123").unwrap()),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let response = t
            .server
            .post("/api/auth/email/resend-verification")
            .json(&json!({ "email": "resend_unverified@example.com" }))
            .await;
        assert_eq!(response.status_code(), 200);
        assert_eq!(t.enqueued_jobs_of_type("send_verification_email").len(), 1);
    }
}
