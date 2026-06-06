use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use sea_orm::TransactionTrait;
use serde::Deserialize;

use crate::{account::purge_user_account, app::App, auth::current_user::CurrentUser, password};

#[derive(Debug, Deserialize)]
pub struct DeleteAccountRequest {
    pub password: String,
}

/// `DELETE /api/account` — permanently delete the authenticated user and all of
/// their data.
///
/// Requires the current password in the body. Framework-owned data (the user
/// row plus cascading `user_tokens` and subscription rows) is deleted
/// synchronously; the Stripe subscription and uploaded files are cleaned up by
/// retryable background jobs. Any registered
/// [`UserDataDeleter`](crate::account::UserDataDeleter) runs in the same
/// transaction.
pub async fn delete_account<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
    Json(body): Json<DeleteAccountRequest>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    // Re-verify identity for this destructive action.
    match password::verify_password(&body.password, &current_user.user.password_hash) {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "Incorrect password" })),
            )
                .into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }

    let txn = match app.db.begin().await {
        Ok(txn) => txn,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if purge_user_account(
        &txn,
        &app.job_queue,
        app.user_data_deleter.as_ref(),
        current_user.user.id,
    )
    .await
    .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    match txn.commit().await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, EntityTrait, Set};
    use serde_json::json;

    use crate::{
        app::App,
        auth::jwt::generate_token,
        database::{migrations::Migrator, models::user},
        password::hash_password,
        storage::delete_user_files_job,
        tests::setup_test::setup_test,
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

    async fn create_user(
        db: &sea_orm::DatabaseConnection,
        email: &str,
        password: &str,
    ) -> user::Model {
        user::ActiveModel {
            email: Set(email.to_string()),
            password_hash: Set(hash_password(password).unwrap()),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_delete_account_requires_auth() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let response = t
            .server
            .delete("/api/account")
            .json(&json!({ "password": "whatever" }))
            .await;
        assert_eq!(response.status_code(), 401);
    }

    #[tokio::test]
    async fn test_delete_account_wrong_password_returns_403() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let u = create_user(&t.db, "del_wrong@example.com", "correct-password").await;
        let token = generate_token(&t.config, u.id, u.token_version).unwrap();

        let response = t
            .server
            .delete("/api/account")
            .add_header("Authorization", format!("Bearer {token}"))
            .json(&json!({ "password": "wrong-password" }))
            .await;
        assert_eq!(response.status_code(), 403);

        // User must still exist.
        assert!(user::Entity::find_by_id(u.id)
            .one(&t.db)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn test_delete_account_deletes_user_and_enqueues_file_cleanup() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let u = create_user(&t.db, "del_ok@example.com", "my-password").await;
        let token = generate_token(&t.config, u.id, u.token_version).unwrap();

        let response = t
            .server
            .delete("/api/account")
            .add_header("Authorization", format!("Bearer {token}"))
            .json(&json!({ "password": "my-password" }))
            .await;
        assert_eq!(response.status_code(), 204);

        // User row is gone.
        assert!(user::Entity::find_by_id(u.id)
            .one(&t.db)
            .await
            .unwrap()
            .is_none());

        // The file-cleanup job was enqueued; no Stripe sub, so no cancel job.
        let file_jobs = t
            .job_queue
            .enqueued_jobs_of_type(delete_user_files_job::JOB_NAME)
            .unwrap();
        assert_eq!(file_jobs.len(), 1);
        assert_eq!(file_jobs[0].arguments["user_id"], json!(u.id));

        // The old access token is now rejected.
        let after = t
            .server
            .post("/api/auth/logout")
            .add_header("Authorization", format!("Bearer {token}"))
            .await;
        assert_eq!(after.status_code(), 401);
    }
}
