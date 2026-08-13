//! Docs: docs/src/content/docs/api/console.md
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr,
    EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, Statement,
    TransactionTrait,
};
use uuid::Uuid;

use crate::{
    account::{purge_user_account, UserDataDeleter},
    admin::dto::{
        AdminEventDto, DashboardResponse, EmailJobStat, EmailListResponse, EmailMessageDto,
        EventsResponse, JobDetailResponse, JobExecutionDto, JobSummary, JobTypeStat, JobsResponse,
        SubscriptionInfo, TableCountDto, TablesResponse, UserDetailResponse, UserListResponse,
        UserSummary,
    },
    billing::{
        handlers::webhooks::update_user_subscription_cache,
        lookup::{load_current_subscription, CurrentSubscription},
        models::{gift_subscription, stripe_subscription, trial_subscription},
    },
    database::models::{
        admin_event, email_message,
        job::{self, Column as JobColumn},
        job_execution,
        job_status::JobStatus,
        oauth_identity,
        user::{self, Column as UserColumn},
    },
    job_queue::JobQueue,
};
use std::sync::Arc;

fn user_summary(u: &user::Model) -> UserSummary {
    UserSummary {
        id: u.id,
        email: u.email.clone(),
        email_verified_at: u.email_verified_at,
        last_active_at: u.last_active_at,
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
    let mut stripe = 0;
    let mut gift = 0;
    let mut trial = 0;
    let mut no_sub = 0;
    let mut total = 0;
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT subscription_type, COUNT(*)::bigint AS count FROM users GROUP BY 1",
        ))
        .await?
    {
        let n: i64 = r.try_get("", "count").unwrap_or(0);
        total += n;
        match r
            .try_get::<Option<String>>("", "subscription_type")
            .ok()
            .flatten()
            .as_deref()
        {
            Some("stripe") => stripe = n,
            Some("gift") => gift = n,
            Some("trial") => trial = n,
            _ => no_sub += n,
        }
    }

    let mut pending = 0;
    let mut running = 0;
    let mut failed = 0;
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT status, COUNT(*)::bigint AS count FROM job GROUP BY 1",
        ))
        .await?
    {
        let n: i64 = r.try_get("", "count").unwrap_or(0);
        match r
            .try_get::<String>("", "status")
            .unwrap_or_default()
            .as_str()
        {
            "pending" | "pending_retry" => pending += n,
            "running" => running = n,
            "failed" => failed = n,
            _ => {}
        }
    }

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
            "SELECT COALESCE(template, '(none)') AS template, \
              COUNT(*)::bigint AS total, \
              COUNT(*) FILTER (WHERE status = 'sent')::bigint AS completed, \
              COUNT(*) FILTER (WHERE status = 'failed')::bigint AS failed \
             FROM email_messages \
             GROUP BY 1 ORDER BY 1",
        ))
        .await?;
    let email_stats: Vec<EmailJobStat> = email_rows
        .into_iter()
        .map(|r| {
            let name: String = r.try_get("", "template").unwrap_or_default();
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
    page: u64,
    per_page: u64,
) -> Result<UserListResponse, DbErr> {
    let page = page.max(1);
    let per_page = per_page.clamp(1, 200);
    let mut q = user::Entity::find().order_by_asc(UserColumn::Email);
    if let Some(qstr) = query.filter(|s| !s.is_empty()) {
        q = q.filter(UserColumn::Email.like(format!("%{}%", qstr.to_lowercase())));
    }
    let total = q.clone().count(db).await?;
    let users = q
        .offset((page - 1) * per_page)
        .limit(per_page)
        .all(db)
        .await?;
    Ok(UserListResponse {
        users: users.iter().map(user_summary).collect(),
        page,
        per_page,
        total,
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

    let oauth_providers = oauth_identity::Entity::find()
        .filter(oauth_identity::Column::UserId.eq(user_id))
        .all(db)
        .await?
        .into_iter()
        .map(|i| i.provider)
        .collect();

    let mut subscription_history = Vec::new();
    for row in stripe_subscription::Entity::find()
        .filter(stripe_subscription::Column::UserId.eq(user_id))
        .order_by_desc(stripe_subscription::Column::CreatedAt)
        .all(db)
        .await?
    {
        subscription_history.push(subscription_info(CurrentSubscription::Stripe(row)));
    }
    for row in gift_subscription::Entity::find()
        .filter(gift_subscription::Column::UserId.eq(user_id))
        .order_by_desc(gift_subscription::Column::CreatedAt)
        .all(db)
        .await?
    {
        subscription_history.push(subscription_info(CurrentSubscription::Gift(row)));
    }
    for row in trial_subscription::Entity::find()
        .filter(trial_subscription::Column::UserId.eq(user_id))
        .order_by_desc(trial_subscription::Column::CreatedAt)
        .all(db)
        .await?
    {
        subscription_history.push(subscription_info(CurrentSubscription::Trial(row)));
    }

    Ok(Some(UserDetailResponse {
        user: user_summary(&u),
        subscription,
        oauth_providers,
        subscription_history,
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
    crate::admin_events::emit_ok(
        db,
        crate::admin_events::USER_VERIFIED,
        Some(user_id),
        serde_json::json!({ "source": "admin" }),
    )
    .await;
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
        Some(plan.clone()),
    )
    .await?;
    crate::admin_events::emit_ok(
        db,
        crate::admin_events::SUBSCRIPTION_GIFTED,
        Some(user_id),
        serde_json::json!({ "plan": plan, "duration_days": duration_days }),
    )
    .await;
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

fn job_summary(j: &job::Model) -> JobSummary {
    JobSummary {
        id: j.id,
        job_type: j.r#type.clone(),
        status: j.status,
        retry_count: j.retry_count,
        created_at: j.created_at,
        next_execution_at: j.next_execution_at,
    }
}

pub async fn job_detail(
    db: &DatabaseConnection,
    job_id: Uuid,
) -> Result<Option<JobDetailResponse>, DbErr> {
    let Some(j) = job::Entity::find_by_id(job_id).one(db).await? else {
        return Ok(None);
    };
    let executions = job_execution::Entity::find()
        .filter(job_execution::Column::JobId.eq(job_id))
        .order_by_desc(job_execution::Column::StartedAt)
        .all(db)
        .await?;
    Ok(Some(JobDetailResponse {
        job: job_summary(&j),
        arguments: j.arguments,
        executions: executions
            .into_iter()
            .map(|e| JobExecutionDto {
                id: e.id,
                result: e.result.to_string(),
                started_at: e.started_at,
                finished_at: e.finished_at,
                execution_time_ms: e.execution_time_ms,
                failure_reason: e.failure_reason,
            })
            .collect(),
    }))
}

fn email_dto(m: email_message::Model) -> EmailMessageDto {
    EmailMessageDto {
        id: m.id,
        to: m.to,
        from: m.from,
        subject: m.subject,
        template: m.template,
        user_id: m.user_id,
        job_id: m.job_id,
        status: m.status,
        error: m.error,
        sent_at: m.sent_at,
        created_at: m.created_at,
    }
}

pub async fn list_emails(
    db: &DatabaseConnection,
    to: Option<&str>,
    template: Option<&str>,
    status: Option<&str>,
    page: u64,
    per_page: u64,
) -> Result<EmailListResponse, DbErr> {
    let page = page.max(1);
    let per_page = per_page.clamp(1, 200);
    let mut q = email_message::Entity::find().order_by_desc(email_message::Column::CreatedAt);
    if let Some(to) = to.filter(|s| !s.is_empty()) {
        q = q.filter(email_message::Column::To.like(format!("%{to}%")));
    }
    if let Some(template) = template.filter(|s| !s.is_empty()) {
        q = q.filter(email_message::Column::Template.eq(template));
    }
    if let Some(status) = status.filter(|s| !s.is_empty()) {
        q = q.filter(email_message::Column::Status.eq(status));
    }
    let total = q.clone().count(db).await?;
    let emails = q
        .offset((page - 1) * per_page)
        .limit(per_page)
        .all(db)
        .await?;
    Ok(EmailListResponse {
        emails: emails.into_iter().map(email_dto).collect(),
        page,
        per_page,
        total,
    })
}

pub async fn get_email(
    db: &DatabaseConnection,
    id: Uuid,
) -> Result<Option<EmailMessageDto>, DbErr> {
    Ok(email_message::Entity::find_by_id(id)
        .one(db)
        .await?
        .map(email_dto))
}

pub async fn list_tables(
    db: &DatabaseConnection,
    configured: &[String],
) -> Result<TablesResponse, DbErr> {
    let pool = db.get_postgres_connection_pool();
    let query = if configured.is_empty() {
        sqlx::query(
            "SELECT relname, n_live_tup, n_dead_tup, \
             COALESCE(last_analyze, last_autoanalyze) AS last_analyze \
             FROM pg_stat_user_tables ORDER BY relname",
        )
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            "SELECT relname, n_live_tup, n_dead_tup, \
             COALESCE(last_analyze, last_autoanalyze) AS last_analyze \
             FROM pg_stat_user_tables WHERE relname = ANY($1) ORDER BY relname",
        )
        .bind(configured)
        .fetch_all(pool)
        .await
    };

    let rows = query.map_err(|e| DbErr::Custom(e.to_string()))?;
    Ok(TablesResponse {
        tables: rows
            .into_iter()
            .map(|r| {
                use sqlx::Row;
                TableCountDto {
                    table: r.try_get::<String, _>(0).unwrap_or_default(),
                    approx_rows: r.try_get::<i64, _>(1).unwrap_or(0),
                    n_dead_tup: r.try_get::<i64, _>(2).unwrap_or(0),
                    last_analyze: r
                        .try_get::<Option<chrono::NaiveDateTime>, _>(3)
                        .ok()
                        .flatten(),
                    approx: true,
                }
            })
            .collect(),
    })
}

pub async fn list_events(
    db: &DatabaseConnection,
    name: Option<&str>,
    days: i64,
    page: u64,
    per_page: u64,
) -> Result<EventsResponse, DbErr> {
    let page = page.max(1);
    let per_page = per_page.clamp(1, 200);
    let days = days.clamp(1, 365);
    let since = Utc::now().naive_utc() - chrono::Duration::days(days);
    let mut q = admin_event::Entity::find()
        .filter(admin_event::Column::CreatedAt.gte(since))
        .order_by_desc(admin_event::Column::CreatedAt);
    if let Some(name) = name.filter(|s| !s.is_empty()) {
        q = q.filter(admin_event::Column::Name.eq(name));
    }
    let events = q
        .offset((page - 1) * per_page)
        .limit(per_page)
        .all(db)
        .await?;
    Ok(EventsResponse {
        events: events
            .into_iter()
            .map(|e| AdminEventDto {
                id: e.id,
                name: e.name,
                user_id: e.user_id,
                payload: e.payload,
                created_at: e.created_at,
            })
            .collect(),
    })
}
