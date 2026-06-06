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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, EntityTrait, Set, TransactionTrait};

    use crate::{
        billing::models::{stripe_subscription, subscription_status::SubscriptionStatus},
        database::migrations::Migrator,
        job_queue::JobQueue,
        password::hash_password,
        tests::setup_test::setup_test,
    };

    fn no_router(_app: crate::app::App) -> axum::Router {
        axum::Router::new()
    }
    fn no_fixtures(
        db: &sea_orm::DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let _ = db;
        })
    }

    async fn create_user(db: &sea_orm::DatabaseConnection, email: &str) -> user::Model {
        user::ActiveModel {
            email: Set(email.to_string()),
            password_hash: Set(hash_password("password123").unwrap()),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    struct RecordingDeleter(Arc<AtomicUsize>);
    #[async_trait]
    impl UserDataDeleter for RecordingDeleter {
        async fn delete_user_data(
            &self,
            _txn: &DatabaseTransaction,
            _user_id: Uuid,
        ) -> Result<(), DbErr> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct FailingDeleter;
    #[async_trait]
    impl UserDataDeleter for FailingDeleter {
        async fn delete_user_data(
            &self,
            _txn: &DatabaseTransaction,
            _user_id: Uuid,
        ) -> Result<(), DbErr> {
            Err(DbErr::Custom("boom".to_string()))
        }
    }

    #[tokio::test]
    async fn purge_invokes_deleter_and_deletes_user() {
        let t = setup_test::<Migrator>(no_router, no_fixtures).await;
        let u = create_user(&t.db, "purge_ok@example.com").await;

        let calls = Arc::new(AtomicUsize::new(0));
        let deleter: Arc<dyn UserDataDeleter> = Arc::new(RecordingDeleter(calls.clone()));
        let queue = JobQueue::mock();

        let txn = t.db.begin().await.unwrap();
        purge_user_account(&txn, &queue, Some(&deleter), u.id)
            .await
            .unwrap();
        txn.commit().await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1, "deleter invoked");
        assert!(user::Entity::find_by_id(u.id)
            .one(&t.db)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            queue
                .enqueued_jobs_of_type(delete_user_files_job::JOB_NAME)
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn purge_aborts_when_deleter_errs() {
        let t = setup_test::<Migrator>(no_router, no_fixtures).await;
        let u = create_user(&t.db, "purge_abort@example.com").await;

        let deleter: Arc<dyn UserDataDeleter> = Arc::new(FailingDeleter);
        let queue = JobQueue::mock();

        let txn = t.db.begin().await.unwrap();
        let result = purge_user_account(&txn, &queue, Some(&deleter), u.id).await;
        assert!(result.is_err());
        // The deleter aborts before any write, so the user delete never runs.
        // (In production the handler drops the txn here, rolling back; the test
        // harness's outer transaction can't nest a rollback, so we commit and
        // verify nothing was deleted — the same invariant.)
        txn.commit().await.unwrap();

        // User survives and nothing was enqueued.
        assert!(user::Entity::find_by_id(u.id)
            .one(&t.db)
            .await
            .unwrap()
            .is_some());
        assert!(queue.enqueued_jobs().unwrap().is_empty());
    }

    #[tokio::test]
    async fn purge_enqueues_stripe_cancel_when_subscription_exists() {
        let t = setup_test::<Migrator>(no_router, no_fixtures).await;
        let u = create_user(&t.db, "purge_stripe@example.com").await;

        let now = Utc::now().naive_utc();
        stripe_subscription::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(u.id),
            stripe_customer_id: Set("cus_123".to_string()),
            stripe_subscription_id: Set("sub_123".to_string()),
            plan: Set("pro".to_string()),
            status: Set(SubscriptionStatus::Active),
            current_period_start: Set(now),
            current_period_end: Set(now),
            cancel_at_period_end: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&t.db)
        .await
        .unwrap();

        let queue = JobQueue::mock();
        let txn = t.db.begin().await.unwrap();
        purge_user_account(&txn, &queue, None, u.id).await.unwrap();
        txn.commit().await.unwrap();

        let cancel_jobs = queue
            .enqueued_jobs_of_type(cancel_stripe_subscription_job::JOB_NAME)
            .unwrap();
        assert_eq!(cancel_jobs.len(), 1);
        assert_eq!(
            cancel_jobs[0].arguments["stripe_subscription_id"],
            "sub_123"
        );
    }
}
