use chrono::{Duration, NaiveDateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Set, Statement,
};

use super::models::stat_snapshot;

/// `(metric, dimension, value)` — one row to insert.
type Metric = (&'static str, Option<String>, f64);

/// Computes the current business-metrics snapshot and inserts one `stat_snapshot`
/// row per metric, all sharing the same `captured_at` timestamp.
///
/// See `docs/src/content/docs/api/business-stats.md` for what each metric means
/// and its known limitations (e.g. hard-deleted accounts leave no trace).
pub async fn compute_and_store_snapshot(db: &DatabaseConnection) -> Result<(), DbErr> {
    let captured_at = Utc::now().naive_utc();

    let mut metrics: Vec<Metric> = vec![
        ("total_users", None, count(db, "SELECT COUNT(*)::bigint AS n FROM users").await? as f64),
        (
            "new_users_since_last",
            None,
            new_users_since_last(db, captured_at).await? as f64,
        ),
        (
            "email_verified_count",
            None,
            count(
                db,
                "SELECT COUNT(*)::bigint AS n FROM users WHERE email_verified_at IS NOT NULL",
            )
            .await? as f64,
        ),
        (
            "paid_active_count",
            None,
            count(
                db,
                "SELECT COUNT(*)::bigint AS n FROM users WHERE subscription_type = 'stripe'",
            )
            .await? as f64,
        ),
        (
            "trial_active_count",
            None,
            count(
                db,
                "SELECT COUNT(*)::bigint AS n FROM users WHERE subscription_type = 'trial'",
            )
            .await? as f64,
        ),
        (
            "gift_active_count",
            None,
            count(
                db,
                "SELECT COUNT(*)::bigint AS n FROM users WHERE subscription_type = 'gift'",
            )
            .await? as f64,
        ),
        (
            "no_sub_count",
            None,
            count(
                db,
                "SELECT COUNT(*)::bigint AS n FROM users WHERE subscription_type IS NULL",
            )
            .await? as f64,
        ),
        (
            "past_due_count",
            None,
            count(
                db,
                "SELECT COUNT(DISTINCT user_id)::bigint AS n FROM stripe_subscriptions WHERE status = 'past_due'",
            )
            .await? as f64,
        ),
        (
            // Cumulative, all-time count of subscription rows ever marked canceled
            // (a user can accumulate multiple stripe_subscriptions rows over time).
            "canceled_count",
            None,
            count(
                db,
                "SELECT COUNT(DISTINCT user_id)::bigint AS n FROM stripe_subscriptions WHERE status = 'canceled'",
            )
            .await? as f64,
        ),
        (
            "cancel_at_period_end_count",
            None,
            count(
                db,
                "SELECT COUNT(*)::bigint AS n FROM stripe_subscriptions WHERE status = 'active' AND cancel_at_period_end = true",
            )
            .await? as f64,
        ),
        (
            "total_storage_bytes",
            None,
            count(
                db,
                "SELECT COALESCE(SUM(byte_size), 0)::bigint AS n FROM files \
                 WHERE id IN (SELECT DISTINCT file_id FROM file_attachments)",
            )
            .await? as f64,
        ),
        (
            "total_file_count",
            None,
            count(
                db,
                "SELECT COUNT(DISTINCT file_id)::bigint AS n FROM file_attachments",
            )
            .await? as f64,
        ),
        (
            "active_users_1d",
            None,
            count(
                db,
                "SELECT COUNT(*)::bigint AS n FROM users WHERE last_active_at > NOW() - INTERVAL '1 day'",
            )
            .await? as f64,
        ),
        (
            "active_users_7d",
            None,
            count(
                db,
                "SELECT COUNT(*)::bigint AS n FROM users WHERE last_active_at > NOW() - INTERVAL '7 days'",
            )
            .await? as f64,
        ),
        (
            "active_users_30d",
            None,
            count(
                db,
                "SELECT COUNT(*)::bigint AS n FROM users WHERE last_active_at > NOW() - INTERVAL '30 days'",
            )
            .await? as f64,
        ),
    ];

    metrics.extend(paid_active_by_plan(db).await?);

    for (metric, dimension, value) in metrics {
        stat_snapshot::ActiveModel {
            captured_at: Set(captured_at),
            metric: Set(metric.to_string()),
            dimension: Set(dimension),
            value: Set(value),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }

    Ok(())
}

/// Runs a fixed `SELECT ... AS n` count/aggregate query and returns the result,
/// defaulting to 0 if the row or column is somehow missing.
async fn count(db: &DatabaseConnection, sql: &'static str) -> Result<i64, DbErr> {
    let row = db
        .query_one(Statement::from_string(DbBackend::Postgres, sql))
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<i64>("", "n").ok())
        .unwrap_or(0))
}

/// New users since the previous run's `total_users` snapshot, or since 24h ago
/// if this is the first run.
async fn new_users_since_last(
    db: &DatabaseConnection,
    captured_at: NaiveDateTime,
) -> Result<i64, DbErr> {
    let previous_row = db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT MAX(captured_at) AS t FROM stat_snapshot WHERE metric = 'total_users'",
        ))
        .await?;
    let since: NaiveDateTime = previous_row
        .and_then(|r| r.try_get::<Option<NaiveDateTime>>("", "t").ok())
        .flatten()
        .unwrap_or(captured_at - Duration::days(1));

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT COUNT(*)::bigint AS n FROM users WHERE created_at > $1",
        [since.into()],
    );
    let row = db.query_one(stmt).await?;
    Ok(row
        .and_then(|r| r.try_get::<i64>("", "n").ok())
        .unwrap_or(0))
}

/// Active Stripe subscriber counts broken down per plan (only plans with at
/// least one active subscriber appear — no zero-filling from config here, that's
/// a display-side concern for the admin TUI).
async fn paid_active_by_plan(db: &DatabaseConnection) -> Result<Vec<Metric>, DbErr> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT subscription_plan AS plan, COUNT(*)::bigint AS n FROM users \
             WHERE subscription_type = 'stripe' AND subscription_plan IS NOT NULL \
             GROUP BY subscription_plan",
        ))
        .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let plan: String = r.try_get("", "plan").ok()?;
            let n: i64 = r.try_get("", "n").ok()?;
            Some(("paid_active_count", Some(plan), n as f64))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{EntityTrait, Set};
    use uuid::Uuid;

    use crate::{
        billing::models::{stripe_subscription, subscription_status::SubscriptionStatus},
        database::migrations::Migrator,
        database::models::user,
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

    #[allow(clippy::too_many_arguments)]
    async fn create_user(
        db: &sea_orm::DatabaseConnection,
        email: &str,
        subscription_type: Option<&str>,
        subscription_plan: Option<&str>,
        last_active_at: Option<NaiveDateTime>,
    ) -> user::Model {
        user::ActiveModel {
            email: Set(email.to_string()),
            password_hash: Set(hash_password("password123").unwrap()),
            email_verified_at: Set(Some(Utc::now().naive_utc())),
            subscription_type: Set(subscription_type.map(str::to_string)),
            subscription_plan: Set(subscription_plan.map(str::to_string)),
            last_active_at: Set(last_active_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn insert_subscription(
        db: &sea_orm::DatabaseConnection,
        user_id: Uuid,
        status: SubscriptionStatus,
    ) {
        let now = Utc::now().naive_utc();
        stripe_subscription::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user_id),
            stripe_customer_id: Set(format!("cus_{user_id}")),
            stripe_subscription_id: Set(format!("sub_{}", Uuid::new_v4())),
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

    /// Runs the job twice, once before and once after inserting fixtures, and
    /// asserts on the *delta* between the two snapshots rather than absolute
    /// counts. `users`/`stripe_subscriptions`/etc. are shared, live tables —
    /// other tests running concurrently against the same database (each in
    /// its own transaction) can shift absolute counts, but the delta across
    /// two back-to-back runs of this test is what actually exercises the
    /// job's query logic.
    #[tokio::test]
    async fn computes_expected_metrics() {
        let t = setup_test::<Migrator>(no_router, no_fixtures).await;

        // `users`/`stripe_subscriptions`/etc. are shared, live tables that other
        // concurrently running tests also write to (each in its own
        // transaction). Under the default READ COMMITTED isolation, a rare but
        // real commit from another test landing between this test's two
        // snapshot calls below would shift the "before"/"after" delta this
        // test asserts on. Pin this transaction's snapshot to its start so
        // later commits from other sessions are simply invisible here — this
        // must be the first statement issued on this transaction.
        t.db.execute(Statement::from_string(
            DbBackend::Postgres,
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        ))
        .await
        .unwrap();

        let now = Utc::now().naive_utc();

        compute_and_store_snapshot(&t.db).await.unwrap();

        create_user(
            &t.db,
            "paid1@example.com",
            Some("stripe"),
            Some("pro"),
            Some(now),
        )
        .await;
        create_user(
            &t.db,
            "paid2@example.com",
            Some("stripe"),
            Some("pro"),
            None,
        )
        .await;
        let trial_user =
            create_user(&t.db, "trial@example.com", Some("trial"), Some("pro"), None).await;
        create_user(&t.db, "free@example.com", None, None, None).await;

        // Historical subscription rows, independent of the cached subscription_type above.
        insert_subscription(&t.db, trial_user.id, SubscriptionStatus::Canceled).await;
        insert_subscription(&t.db, trial_user.id, SubscriptionStatus::PastDue).await;

        compute_and_store_snapshot(&t.db).await.unwrap();

        let rows = stat_snapshot::Entity::find().all(&t.db).await.unwrap();

        // Each call shares one `captured_at` across all its metric rows, so the
        // two runs partition into exactly two distinct timestamps. Grouping by
        // that (rather than first/last-seen-in-iteration-order) correctly
        // defaults a metric to 0 for whichever specific run didn't emit a row
        // for it (e.g. a per-plan breakdown is only emitted for plans with at
        // least one active subscriber).
        let mut captured_ats: Vec<NaiveDateTime> = rows.iter().map(|r| r.captured_at).collect();
        captured_ats.sort();
        captured_ats.dedup();
        assert_eq!(
            captured_ats.len(),
            2,
            "expected exactly two snapshot runs, got {captured_ats:?}"
        );
        let (baseline_at, after_at) = (captured_ats[0], captured_ats[1]);

        let value_at = |at: NaiveDateTime, name: &str, dim: Option<&str>| {
            rows.iter()
                .find(|r| r.captured_at == at && r.metric == name && r.dimension.as_deref() == dim)
                .map_or(0.0, |r| r.value)
        };
        let delta = |name: &str, dim: Option<&str>| {
            value_at(after_at, name, dim) - value_at(baseline_at, name, dim)
        };

        assert_eq!(delta("total_users", None), 4.0);
        assert_eq!(delta("paid_active_count", None), 2.0);
        assert_eq!(delta("paid_active_count", Some("pro")), 2.0);
        assert_eq!(delta("trial_active_count", None), 1.0);
        assert_eq!(delta("no_sub_count", None), 1.0);
        assert_eq!(delta("canceled_count", None), 1.0);
        assert_eq!(delta("past_due_count", None), 1.0);
        assert_eq!(delta("cancel_at_period_end_count", None), 0.0);
        assert_eq!(delta("active_users_1d", None), 1.0);
        assert_eq!(delta("active_users_7d", None), 1.0);
        assert_eq!(delta("total_file_count", None), 0.0);
    }
}
