//! Account deletion — framework-owned data wipe plus an app extension seam.
//!
//! Docs: docs/src/content/docs/api/authentication.md

use std::sync::Arc;

use async_trait::async_trait;
use sea_orm::{ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::{
    billing::{jobs::cancel_stripe_subscription_job, models::stripe_subscription},
    database::models::user,
    job_queue::JobQueue,
    storage::delete_user_files_job,
};

/// Hook for deleting app-owned, per-user data during account deletion.
///
/// Register an implementation via
/// [`BootConfig::on_delete_user`](crate::boot::BootConfig::on_delete_user). Apps
/// that prefer SQL-level cleanup can instead give their per-user tables an
/// `ON DELETE CASCADE` foreign key to `users` and skip this hook — both work.
#[async_trait]
pub trait UserDataDeleter: Send + Sync + 'static {
    /// Delete every row your app owns for `user_id`. Runs inside the deletion
    /// transaction, before the framework removes the `users` row; returning
    /// `Err` aborts and rolls back the whole deletion.
    async fn delete_user_data(&self, txn: &DatabaseTransaction, user_id: Uuid)
        -> Result<(), DbErr>;
}

/// Hard-delete everything Erno owns for `user_id` inside `txn`, then enqueue the
/// external-cleanup jobs (Stripe cancellation + file removal). The caller is
/// responsible for committing `txn`.
///
/// Deleting the `users` row cascades to `user_tokens` and the stripe/trial/gift
/// subscription tables. Files (`file_attachments` with `record_type = "user"`)
/// have no user foreign key, so they are removed asynchronously by
/// [`DeleteUserFilesJob`](crate::storage::delete_user_files_job::DeleteUserFilesJob)
/// keyed on `user_id`.
pub async fn purge_user_account(
    txn: &DatabaseTransaction,
    job_queue: &JobQueue,
    deleter: Option<&Arc<dyn UserDataDeleter>>,
    user_id: Uuid,
) -> Result<(), DbErr> {
    // Capture the Stripe subscription id before the cascade removes the row.
    let stripe_subscription_id = stripe_subscription::Entity::find()
        .filter(stripe_subscription::Column::UserId.eq(user_id))
        .order_by_desc(stripe_subscription::Column::CreatedAt)
        .one(txn)
        .await?
        .map(|s| s.stripe_subscription_id);

    // App-owned data first, so a failure aborts before we touch framework data.
    if let Some(deleter) = deleter {
        deleter.delete_user_data(txn, user_id).await?;
    }

    // Framework-owned data: cascades to user_tokens + subscription tables.
    user::Entity::delete_by_id(user_id).exec(txn).await?;

    // External cleanup is enqueued in the same transaction so it is atomic with
    // the delete, and runs asynchronously with retries.
    if let Some(subscription_id) = stripe_subscription_id {
        job_queue
            .enqueue_by_name(
                txn,
                cancel_stripe_subscription_job::JOB_NAME,
                serde_json::json!({ "stripe_subscription_id": subscription_id }),
            )
            .await?;
    }
    job_queue
        .enqueue_by_name(
            txn,
            delete_user_files_job::JOB_NAME,
            serde_json::json!({ "user_id": user_id }),
        )
        .await?;

    Ok(())
}
