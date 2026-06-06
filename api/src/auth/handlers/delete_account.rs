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
