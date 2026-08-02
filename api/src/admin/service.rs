//! Docs: docs/src/content/docs/api/console.md
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr,
    EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set, Statement, TransactionTrait,
};
use uuid::Uuid;

use crate::{
    account::{purge_user_account, UserDataDeleter},
    admin::dto::{
        DashboardResponse, EmailJobStat, JobSummary, JobTypeStat, JobsResponse, MetricPointDto,
        MetricSeriesDto, StatsResponse, SubscriptionInfo, UserDetailResponse, UserListResponse,
        UserSummary,
    },
    billing::{
        handlers::webhooks::update_user_subscription_cache,
        lookup::{load_current_subscription, CurrentSubscription},
        models::gift_subscription,
    },
    business_stats::models::stat_snapshot,
    database::models::{
        job::{self, Column as JobColumn},
        job_status::JobStatus,
        user::{self, Column as UserColumn},
    },
    job_queue::JobQueue,
};
use std::sync::Arc;

/// Display order for well-known metrics (matches the original Stats TUI).
const HEADLINE_METRICS: &[&str] = &[
    "total_users",
    "new_users_since_last",
    "email_verified_count",
    "paid_active_count",
    "trial_active_count",
    "gift_active_count",
    "no_sub_count",
    "past_due_count",
    "canceled_count",
    "cancel_at_period_end_count",
    "active_users_1d",
    "active_users_7d",
    "active_users_30d",
    "total_storage_bytes",
    "total_file_count",
];

fn user_summary(u: &user::Model) -> UserSummary {
    UserSummary {
        id: u.id,
        email: u.email.clone(),
        email_verified_at: u.email_verified_at,
        subscription_type: u.subscription_type.clone(),
        subscription_plan: u.subscription_plan.clone(),
        created_at: u.created_at,
    }
}

fn subscription_info(s: CurrentSubscription) -> SubscriptionInfo {
    match s {
        CurrentSubscription::Stripe(m) => SubscriptionInfo {
            sub_type: "Stripe".to_string(),
            plan: m.plan.clone(),
            status: format!("{:?}", m.status),
            expiry: m.current_period_end.format("%Y-%m-%d").to_string(),
            stripe_customer_id: Some(m.stripe_customer_id),
            stripe_sub_id: Some(m.stripe_subscription_id),
            cancel_at_period_end: Some(m.cancel_at_period_end),
        },
        CurrentSubscription::Gift(m) => SubscriptionInfo {
            sub_type: "Gift".to_string(),
            plan: m.plan,
            status: "Active".to_string(),
            expiry: m.active_until.format("%Y-%m-%d").to_string(),
            stripe_customer_id: None,
            stripe_sub_id: None,
            cancel_at_period_end: None,
        },
        CurrentSubscription::Trial(m) => SubscriptionInfo {
            sub_type: "Trial".to_string(),
            plan: m.plan,
            status: "Active".to_string(),
            expiry: m.active_until.format("%Y-%m-%d").to_string(),
            stripe_customer_id: None,
            stripe_sub_id: None,
            cancel_at_period_end: None,
        },
    }
}

pub async fn dashboard(db: &DatabaseConnection) -> Result<DashboardResponse, DbErr> {
    let count_from = |sql: &'static str| async move {
        db.query_one(Statement::from_string(DbBackend::Postgres, sql))
            .await
            .map(|r| r.and_then(|r| r.try_get::<i64>("", "count").ok()).unwrap_or(0))
    };

    let total = count_from("SELECT COUNT(*)::bigint AS count FROM users").await?;
    let stripe = count_from(
        "SELECT COUNT(*)::bigint AS count FROM users WHERE subscription_type = 'stripe'",
    )
    .await?;
    let gift =
        count_from("SELECT COUNT(*)::bigint AS count FROM users WHERE subscription_type = 'gift'")
            .await?;
    let trial =
        count_from("SELECT COUNT(*)::bigint AS count FROM users WHERE subscription_type = 'trial'")
            .await?;
    let no_sub =
        count_from("SELECT COUNT(*)::bigint AS count FROM users WHERE subscription_type IS NULL")
            .await?;
    let pending = count_from(
        "SELECT COUNT(*)::bigint AS count FROM job WHERE status IN ('pending', 'pending_retry')",
    )
    .await?;
    let running =
        count_from("SELECT COUNT(*)::bigint AS count FROM job WHERE status = 'running'").await?;
    let failed =
        count_from("SELECT COUNT(*)::bigint AS count FROM job WHERE status = 'failed'").await?;

    let exec_row = db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT \
              COUNT(*) FILTER (WHERE result = 'completed')::bigint AS completed, \
              COUNT(*) FILTER (WHERE result = 'failed')::bigint    AS failed, \
              COUNT(*) FILTER (WHERE result = 'timed_out')::bigint AS timed_out, \
              COALESCE(AVG(execution_time_ms) FILTER (WHERE result = 'completed'), 0)::bigint AS avg_ms \
             FROM job_execution \
             WHERE finished_at > NOW() - INTERVAL '1 hour'",
        ))
        .await?;
    let (completed_1h, failed_1h, timed_out_1h, avg_ms) = exec_row
        .map(|r| {
            let c: i64 = r.try_get("", "completed").unwrap_or(0);
            let f: i64 = r.try_get("", "failed").unwrap_or(0);
            let t: i64 = r.try_get("", "timed_out").unwrap_or(0);
            let a: i64 = r.try_get("", "avg_ms").unwrap_or(0);
            (c, f, t, a)
        })
        .unwrap_or((0, 0, 0, 0));

    let email_rows = db
        .query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT j.type, \
              COUNT(*)::bigint AS total, \
              COUNT(*) FILTER (WHERE je.result = 'completed')::bigint AS completed, \
              COUNT(*) FILTER (WHERE je.result = 'failed')::bigint    AS failed \
             FROM job_execution je \
             JOIN job j ON je.job_id = j.id \
             WHERE j.type IN ('send_verification_email','send_password_reset_email','send_already_registered_email') \
             GROUP BY j.type ORDER BY j.type",
        ))
        .await?;
    let email_stats: Vec<EmailJobStat> = email_rows
        .into_iter()
        .map(|r| {
            let raw: String = r.try_get("", "type").unwrap_or_default();
            let name = raw
                .strip_prefix("send_")
                .and_then(|s| s.strip_suffix("_email"))
                .unwrap_or(&raw)
                .to_string();
            let total: i64 = r.try_get("", "total").unwrap_or(0);
            let completed: i64 = r.try_get("", "completed").unwrap_or(0);
            let failed: i64 = r.try_get("", "failed").unwrap_or(0);
            EmailJobStat {
                name,
                total,
                completed,
                failed,
            }
        })
        .collect();

    Ok(DashboardResponse {
        total_users: total,
        stripe_active: stripe,
        gift_active: gift,
        trial_active: trial,
        no_sub,
        pending_jobs: pending,
        running_jobs: running,
        failed_jobs: failed,
        completed_jobs_1h: completed_1h,
        failed_executions_1h: failed_1h,
        timed_out_1h,
        avg_execution_ms: avg_ms,
        email_stats,
        refreshed_at: Utc::now().naive_utc(),
    })
}

pub async fn list_users(
    db: &DatabaseConnection,
    query: Option<&str>,
) -> Result<UserListResponse, DbErr> {
    let mut q = user::Entity::find().order_by_asc(UserColumn::Email);
    if let Some(qstr) = query.filter(|s| !s.is_empty()) {
        q = q.filter(UserColumn::Email.like(format!("%{}%", qstr.to_lowercase())));
    }
    let users = q.limit(200).all(db).await?;
    Ok(UserListResponse {
        users: users.iter().map(user_summary).collect(),
    })
}

pub async fn user_detail(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<Option<UserDetailResponse>, DbErr> {
    let Some(u) = user::Entity::find_by_id(user_id).one(db).await? else {
        return Ok(None);
    };
    let subscription = load_current_subscription(db, &u)
        .await
        .map(subscription_info);
    Ok(Some(UserDetailResponse {
        user: user_summary(&u),
        subscription,
    }))
}

pub async fn activate_user(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<Option<UserDetailResponse>, DbErr> {
    let Some(_) = user::Entity::find_by_id(user_id).one(db).await? else {
        return Ok(None);
    };
    let now = Utc::now().naive_utc();
    let active = user::ActiveModel {
        id: Set(user_id),
        email_verified_at: Set(Some(now)),
        ..Default::default()
    };
    user::Entity::update(active).exec(db).await?;
    user_detail(db, user_id).await
}

pub async fn delete_user(
    db: &DatabaseConnection,
    job_queue: &JobQueue,
    deleter: Option<&Arc<dyn UserDataDeleter>>,
    user_id: Uuid,
) -> Result<bool, DbErr> {
    let Some(_) = user::Entity::find_by_id(user_id).one(db).await? else {
        return Ok(false);
    };
    let txn = db.begin().await?;
    purge_user_account(&txn, job_queue, deleter, user_id).await?;
    txn.commit().await?;
    Ok(true)
}

pub async fn gift_subscription(
    db: &DatabaseConnection,
    user_id: Uuid,
    plan: String,
    duration_days: u32,
) -> Result<Option<UserDetailResponse>, DbErr> {
    let Some(_) = user::Entity::find_by_id(user_id).one(db).await? else {
        return Ok(None);
    };
    let active_until = Utc::now().naive_utc() + chrono::Duration::days(i64::from(duration_days));
    let now = Utc::now().naive_utc();
    let row = gift_subscription::ActiveModel {
        user_id: Set(user_id),
        plan: Set(plan.clone()),
        active_until: Set(active_until),
        created_at: Set(now),
        ..Default::default()
    };
    let inserted = row.insert(db).await?;
    update_user_subscription_cache(
        db,
        user_id,
        Some(inserted.id),
        Some("gift".to_string()),
        Some(plan),
    )
    .await?;
    user_detail(db, user_id).await
}

pub async fn list_jobs(
    db: &DatabaseConnection,
    status: Option<JobStatus>,
    job_type: Option<&str>,
) -> Result<JobsResponse, DbErr> {
    let stats_rows = db
        .query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT type, \
             SUM(CASE WHEN status IN ('pending','pending_retry') THEN 1 ELSE 0 END) AS pending, \
             SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END) AS running, \
             SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed, \
             SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) AS completed \
             FROM job GROUP BY type ORDER BY type",
        ))
        .await?;

    let stats: Vec<JobTypeStat> = stats_rows
        .into_iter()
        .map(|r| {
            let t: String = r.try_get("", "type").unwrap_or_default();
            let p: i64 = r.try_get("", "pending").unwrap_or(0);
            let ru: i64 = r.try_get("", "running").unwrap_or(0);
            let f: i64 = r.try_get("", "failed").unwrap_or(0);
            let c: i64 = r.try_get("", "completed").unwrap_or(0);
            JobTypeStat {
                job_type: t,
                pending: p,
                running: ru,
                failed: f,
                completed: c,
            }
        })
        .collect();

    let mut q = job::Entity::find().order_by_desc(JobColumn::CreatedAt);
    if let Some(s) = status {
        q = q.filter(JobColumn::Status.eq(s));
    }
    if let Some(t) = job_type.filter(|s| !s.is_empty()) {
        q = q.filter(JobColumn::Type.eq(t));
    }
    let jobs = q.limit(100).all(db).await?;

    Ok(JobsResponse {
        stats,
        jobs: jobs
            .into_iter()
            .map(|j| JobSummary {
                id: j.id,
                job_type: j.r#type,
                status: j.status,
                retry_count: j.retry_count,
                created_at: j.created_at,
                next_execution_at: j.next_execution_at,
            })
            .collect(),
    })
}

pub async fn retry_job(db: &DatabaseConnection, job_id: Uuid) -> Result<bool, DbErr> {
    let Some(_existing) = job::Entity::find_by_id(job_id).one(db).await? else {
        return Ok(false);
    };
    let active = job::ActiveModel {
        id: Set(job_id),
        status: Set(JobStatus::Pending),
        next_execution_at: Set(None),
        ..Default::default()
    };
    job::Entity::update(active).exec(db).await?;
    Ok(true)
}

/// Load `stat_snapshot` history for the business-stats sparklines.
pub async fn business_stats(
    db: &DatabaseConnection,
    window_days: i64,
) -> Result<StatsResponse, DbErr> {
    let window_days = window_days.clamp(1, 365);
    let since = Utc::now().naive_utc() - chrono::Duration::days(window_days);

    let rows = stat_snapshot::Entity::find()
        .filter(stat_snapshot::Column::CapturedAt.gte(since))
        .order_by_asc(stat_snapshot::Column::Metric)
        .order_by_asc(stat_snapshot::Column::Dimension)
        .order_by_asc(stat_snapshot::Column::CapturedAt)
        .all(db)
        .await?;

    Ok(StatsResponse {
        window_days,
        series: group_into_series(rows),
    })
}

fn group_into_series(rows: Vec<stat_snapshot::Model>) -> Vec<MetricSeriesDto> {
    type Grouped =
        std::collections::BTreeMap<(String, Option<String>), Vec<(chrono::NaiveDateTime, f64)>>;

    let mut grouped: Grouped = Grouped::new();
    for row in rows {
        grouped
            .entry((row.metric, row.dimension))
            .or_default()
            .push((row.captured_at, row.value));
    }

    let mut headline = Vec::new();
    let mut rest = Vec::new();
    for ((metric, dimension), points) in grouped {
        let headline_pos = if dimension.is_none() {
            HEADLINE_METRICS.iter().position(|&m| m == metric)
        } else {
            None
        };
        let label = display_label(&metric, dimension.as_deref());
        let series = MetricSeriesDto {
            metric,
            dimension,
            label,
            points: points
                .into_iter()
                .map(|(captured_at, value)| MetricPointDto { captured_at, value })
                .collect(),
        };
        match headline_pos {
            Some(pos) => headline.push((pos, series)),
            None => rest.push(series),
        }
    }
    headline.sort_by_key(|(pos, _)| *pos);
    headline.into_iter().map(|(_, s)| s).chain(rest).collect()
}

fn display_label(metric: &str, dimension: Option<&str>) -> String {
    let words: Vec<String> = metric
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect();
    let base = words.join(" ");
    match dimension {
        Some(d) => format!("{base} ({d})"),
        None => base,
    }
}
