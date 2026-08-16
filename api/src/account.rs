//! Account deletion — framework-owned data wipe plus an app extension seam.
//!
//! Docs: docs/src/content/docs/api/authentication.md

use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use sea_orm::{ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::{
    billing::{
        jobs::{cancel_stripe_subscription_job, delete_stripe_customer_job},
        models::{stripe_subscription, subscription_status::SubscriptionStatus},
    },
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
///
/// **Files:** a cascade removes your rows but *not* file attachments or stored
/// blobs (`file_attachments` has no FK to your tables). Enqueue
/// [`DeleteRecordAttachmentsJob`](crate::storage::delete_record_attachments_job)
/// for each of your record ids via the provided `job_queue` — enqueues inside
/// `txn`, so they are atomic with the deletion.
#[async_trait]
pub trait UserDataDeleter: Send + Sync + 'static {
    /// Delete every row your app owns for `user_id`. Runs inside the deletion
    /// transaction, before the framework removes the `users` row; returning
    /// `Err` aborts and rolls back the whole deletion.
    async fn delete_user_data(
        &self,
        txn: &DatabaseTransaction,
        job_queue: &JobQueue,
        user_id: Uuid,
    ) -> Result<(), DbErr>;
}

/// Hard-delete everything Erno owns for `user_id` inside `txn`, then enqueue the
/// external-cleanup jobs (Stripe cancellation + customer deletion + file
/// removal). The caller is responsible for committing `txn`.
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
    // Capture the Stripe ids before the cascade removes the rows: *every*
    // active/past-due subscription must be cancelled (a user can have several
    // rows — one per checkout) and every customer object deleted, or a deleted
    // user keeps getting billed / retains PII at Stripe.
    let subscriptions = stripe_subscription::Entity::find()
        .filter(stripe_subscription::Column::UserId.eq(user_id))
        .all(txn)
        .await?;

    // App-owned data first, so a failure aborts before we touch framework data.
    if let Some(deleter) = deleter {
        deleter.delete_user_data(txn, job_queue, user_id).await?;
    }

    crate::admin_events::emit(
        txn,
        crate::admin_events::USER_DELETED,
        Some(user_id),
        crate::admin_events::empty_payload(),
    )
    .await?;

    // Framework-owned data: cascades to user_tokens + subscription tables.
    user::Entity::delete_by_id(user_id).exec(txn).await?;

    // External cleanup is enqueued in the same transaction so it is atomic with
    // the delete, and runs asynchronously with retries.
    let mut customer_ids = HashSet::new();
    for subscription in &subscriptions {
        if matches!(
            subscription.status,
            SubscriptionStatus::Active | SubscriptionStatus::PastDue
        ) {
            job_queue
                .enqueue_by_name(
                    txn,
                    cancel_stripe_subscription_job::JOB_NAME,
                    serde_json::json!({
                        "stripe_subscription_id": subscription.stripe_subscription_id
                    }),
                )
                .await?;
        }
        customer_ids.insert(&subscription.stripe_customer_id);
    }
    for customer_id in customer_ids {
        job_queue
            .enqueue_by_name(
                txn,
                delete_stripe_customer_job::JOB_NAME,
                serde_json::json!({ "stripe_customer_id": customer_id }),
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
        billing::{
            jobs::delete_stripe_customer_job,
            models::{stripe_subscription, subscription_status::SubscriptionStatus},
        },
        database::migrations::Migrator,
        job_queue::JobQueue,
        password::hash_password,
        tests::setup_test::{setup_test, test_boot},
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
            password_hash: Set(Some(hash_password("password123").unwrap())),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn insert_subscription(
        db: &sea_orm::DatabaseConnection,
        user_id: Uuid,
        customer_id: &str,
        subscription_id: &str,
        status: SubscriptionStatus,
    ) {
        let now = Utc::now().naive_utc();
        stripe_subscription::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user_id),
            stripe_customer_id: Set(customer_id.to_string()),
            stripe_subscription_id: Set(subscription_id.to_string()),
            plan: Set("pro".to_string()),
            status: Set(status),
            current_period_start: Set(now),
            current_period_end: Set(now),
            cancel_at_period_end: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap();
    }

    struct RecordingDeleter(Arc<AtomicUsize>);
    #[async_trait]
    impl UserDataDeleter for RecordingDeleter {
        async fn delete_user_data(
            &self,
            _txn: &DatabaseTransaction,
            _job_queue: &JobQueue,
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
            _job_queue: &JobQueue,
            _user_id: Uuid,
        ) -> Result<(), DbErr> {
            Err(DbErr::Custom("boom".to_string()))
        }
    }

    /// Writes a row (a `files` row, as a stand-in for app-owned data) and then
    /// fails — used to prove the partial write is rolled back.
    struct WriteThenFailDeleter;
    #[async_trait]
    impl UserDataDeleter for WriteThenFailDeleter {
        async fn delete_user_data(
            &self,
            txn: &DatabaseTransaction,
            _job_queue: &JobQueue,
            user_id: Uuid,
        ) -> Result<(), DbErr> {
            use crate::storage::models::file;
            let now = Utc::now().naive_utc();
            file::ActiveModel {
                id: Set(user_id), // reuse user_id so the test can find it
                key: Set(format!("partial/{user_id}")),
                filename: Set("partial".to_string()),
                content_type: Set(None),
                byte_size: Set(0),
                checksum: Set("x".to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(txn)
            .await?;
            Err(DbErr::Custom("boom after write".to_string()))
        }
    }

    #[tokio::test]
    async fn purge_invokes_deleter_and_deletes_user() {
        let t = setup_test::<Migrator, _>(test_boot(no_router), no_fixtures).await;
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
        let t = setup_test::<Migrator, _>(test_boot(no_router), no_fixtures).await;
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
        let t = setup_test::<Migrator, _>(test_boot(no_router), no_fixtures).await;
        let u = create_user(&t.db, "purge_stripe@example.com").await;

        insert_subscription(
            &t.db,
            u.id,
            "cus_123",
            "sub_123",
            SubscriptionStatus::Active,
        )
        .await;

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

        // The customer object is deleted too.
        let customer_jobs = queue
            .enqueued_jobs_of_type(delete_stripe_customer_job::JOB_NAME)
            .unwrap();
        assert_eq!(customer_jobs.len(), 1);
        assert_eq!(customer_jobs[0].arguments["stripe_customer_id"], "cus_123");
    }

    #[tokio::test]
    async fn purge_cancels_every_active_subscription_not_just_the_newest() {
        let t = setup_test::<Migrator, _>(test_boot(no_router), no_fixtures).await;
        let u = create_user(&t.db, "purge_multi@example.com").await;

        // Two live subscriptions (one per checkout) plus an already-canceled
        // row, spread across two customer objects.
        insert_subscription(&t.db, u.id, "cus_1", "sub_old", SubscriptionStatus::Active).await;
        insert_subscription(&t.db, u.id, "cus_2", "sub_new", SubscriptionStatus::PastDue).await;
        insert_subscription(
            &t.db,
            u.id,
            "cus_2",
            "sub_dead",
            SubscriptionStatus::Canceled,
        )
        .await;

        let queue = JobQueue::mock();
        let txn = t.db.begin().await.unwrap();
        purge_user_account(&txn, &queue, None, u.id).await.unwrap();
        txn.commit().await.unwrap();

        // Both live subscriptions get a cancel job; the canceled one doesn't.
        let cancel_jobs = queue
            .enqueued_jobs_of_type(cancel_stripe_subscription_job::JOB_NAME)
            .unwrap();
        let cancelled: Vec<&str> = cancel_jobs
            .iter()
            .filter_map(|j| j.arguments["stripe_subscription_id"].as_str())
            .collect();
        assert_eq!(cancelled.len(), 2);
        assert!(cancelled.contains(&"sub_old"));
        assert!(cancelled.contains(&"sub_new"));

        // Both customer objects get a delete job, once each.
        let customer_jobs = queue
            .enqueued_jobs_of_type(delete_stripe_customer_job::JOB_NAME)
            .unwrap();
        let customers: Vec<&str> = customer_jobs
            .iter()
            .filter_map(|j| j.arguments["stripe_customer_id"].as_str())
            .collect();
        assert_eq!(customers.len(), 2);
        assert!(customers.contains(&"cus_1"));
        assert!(customers.contains(&"cus_2"));
    }

    #[tokio::test]
    async fn purge_rolls_back_partial_deleter_writes_on_error() {
        let t = setup_test::<Migrator, _>(test_boot(no_router), no_fixtures).await;
        let u = create_user(&t.db, "purge_rollback@example.com").await;

        let deleter: Arc<dyn UserDataDeleter> = Arc::new(WriteThenFailDeleter);
        let queue = JobQueue::mock();

        // The test harness already runs inside a manually-BEGINed transaction,
        // so the first begin() is a no-op BEGIN and the *nested* begin() is a
        // real savepoint — which can be rolled back, letting us verify the
        // partial write is undone, not just that we returned early.
        let txn = t.db.begin().await.unwrap();
        let sp = txn.begin().await.unwrap();
        let result = purge_user_account(&sp, &queue, Some(&deleter), u.id).await;
        assert!(result.is_err());
        sp.rollback().await.unwrap();

        // Assert through the still-open transaction: the pool has a single
        // connection, so `t.db` can't be queried until `txn` is done.
        // The deleter's row must be gone, the user must survive, nothing enqueued.
        assert!(crate::storage::models::file::Entity::find_by_id(u.id)
            .one(&txn)
            .await
            .unwrap()
            .is_none());
        assert!(user::Entity::find_by_id(u.id)
            .one(&txn)
            .await
            .unwrap()
            .is_some());
        assert!(queue.enqueued_jobs().unwrap().is_empty());
    }
}
