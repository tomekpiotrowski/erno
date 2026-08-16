use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sea_orm::TransactionTrait;

use crate::{account::purge_user_account, app::App, auth::current_user::CurrentUser, password};

/// Header carrying the user's current password for confirmation. A header
/// (not a JSON body) because some proxies/CDNs strip bodies on DELETE.
pub const CONFIRM_PASSWORD_HEADER: &str = "x-confirm-password";

/// `DELETE /api/account` — permanently delete the authenticated user and all of
/// their data.
///
/// Requires the current password in the `X-Confirm-Password` header.
/// Framework-owned data (the user row plus cascading `user_tokens` and
/// subscription rows) is deleted synchronously; Stripe subscriptions, the
/// Stripe customer, and uploaded files are cleaned up by retryable background
/// jobs. Any registered [`UserDataDeleter`](crate::account::UserDataDeleter)
/// runs in the same transaction.
pub async fn delete_account<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    current_user: CurrentUser,
    headers: HeaderMap,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    // Password accounts must re-confirm with the current password. OAuth-only
    // accounts (null password_hash) may delete with a valid access token alone.
    if let Some(password_hash) = current_user.user.password_hash.as_deref() {
        let Some(password) = headers
            .get(CONFIRM_PASSWORD_HEADER)
            .and_then(|v| v.to_str().ok())
        else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("Missing {CONFIRM_PASSWORD_HEADER} header")
                })),
            )
                .into_response();
        };

        match password::verify_password(password, password_hash) {
            Ok(true) => {}
            Ok(false) => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({ "error": "Incorrect password" })),
                )
                    .into_response()
            }
            Err(e) => {
                tracing::error!("Account deletion: password verification failed: {e}");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    }

    let user_id = current_user.user.id;

    let txn = match app.db.begin().await {
        Ok(txn) => txn,
        Err(e) => {
            tracing::error!("Account deletion: failed to begin transaction: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let Err(e) = purge_user_account(
        &txn,
        &app.job_queue,
        app.user_data_deleter.as_ref(),
        user_id,
    )
    .await
    {
        tracing::error!("Account deletion: purge failed for user {user_id}: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    if let Err(e) = txn.commit().await {
        tracing::error!("Account deletion: commit failed for user {user_id}: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Close any live WebSocket connections so the deleted user's sockets don't
    // linger receiving sync pushes.
    app.websocket_connections.disconnect_user(user_id).await;

    StatusCode::NO_CONTENT.into_response()
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
        tests::setup_test::{setup_test, test_boot},
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
            password_hash: Set(Some(hash_password(password).unwrap())),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_delete_account_requires_auth() {
        let t = setup_test::<Migrator, _>(test_boot(test_router), no_fixtures).await;
        let response = t
            .server
            .delete("/api/account")
            .add_header("X-Confirm-Password", "whatever")
            .await;
        assert_eq!(response.status_code(), 401);
    }

    #[tokio::test]
    async fn test_delete_account_missing_password_header_returns_400() {
        let t = setup_test::<Migrator, _>(test_boot(test_router), no_fixtures).await;
        let u = create_user(&t.db, "del_noheader@example.com", "correct-password").await;
        let token = generate_token(&t.config, u.id, u.token_version).unwrap();

        let response = t
            .server
            .delete("/api/account")
            .add_header("Authorization", format!("Bearer {token}"))
            .await;
        assert_eq!(response.status_code(), 400);

        // User must still exist.
        assert!(user::Entity::find_by_id(u.id)
            .one(&t.db)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn test_delete_account_wrong_password_returns_403() {
        let t = setup_test::<Migrator, _>(test_boot(test_router), no_fixtures).await;
        let u = create_user(&t.db, "del_wrong@example.com", "correct-password").await;
        let token = generate_token(&t.config, u.id, u.token_version).unwrap();

        let response = t
            .server
            .delete("/api/account")
            .add_header("Authorization", format!("Bearer {token}"))
            .add_header("X-Confirm-Password", "wrong-password")
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
        let t = setup_test::<Migrator, _>(test_boot(test_router), no_fixtures).await;
        let u = create_user(&t.db, "del_ok@example.com", "my-password").await;
        let token = generate_token(&t.config, u.id, u.token_version).unwrap();

        let response = t
            .server
            .delete("/api/account")
            .add_header("Authorization", format!("Bearer {token}"))
            .add_header("X-Confirm-Password", "my-password")
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
