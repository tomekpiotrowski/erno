//! Docs: docs/src/content/docs/api/console.md
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::models::job_status::JobStatus;

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct DashboardResponse {
    pub total_users: i64,
    pub stripe_active: i64,
    pub gift_active: i64,
    pub trial_active: i64,
    pub no_sub: i64,
    pub pending_jobs: i64,
    pub running_jobs: i64,
    pub failed_jobs: i64,
    pub completed_jobs_1h: i64,
    pub failed_executions_1h: i64,
    pub timed_out_1h: i64,
    pub avg_execution_ms: i64,
    pub email_stats: Vec<EmailJobStat>,
    pub refreshed_at: NaiveDateTime,
}

#[derive(Debug, Serialize)]
pub struct EmailJobStat {
    pub name: String,
    pub total: i64,
    pub completed: i64,
    pub failed: i64,
}

#[derive(Debug, Serialize)]
pub struct UserSummary {
    pub id: Uuid,
    pub email: String,
    pub email_verified_at: Option<NaiveDateTime>,
    pub subscription_type: Option<String>,
    pub subscription_plan: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Serialize)]
pub struct UserListResponse {
    pub users: Vec<UserSummary>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionInfo {
    pub sub_type: String,
    pub plan: String,
    pub status: String,
    pub expiry: String,
    pub stripe_customer_id: Option<String>,
    pub stripe_sub_id: Option<String>,
    pub cancel_at_period_end: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct UserDetailResponse {
    pub user: UserSummary,
    pub subscription: Option<SubscriptionInfo>,
}

#[derive(Debug, Deserialize)]
pub struct GiftRequest {
    pub plan: String,
    pub duration_days: u32,
}

#[derive(Debug, Serialize)]
pub struct JobTypeStat {
    pub job_type: String,
    pub pending: i64,
    pub running: i64,
    pub failed: i64,
    pub completed: i64,
}

#[derive(Debug, Serialize)]
pub struct JobSummary {
    pub id: Uuid,
    pub job_type: String,
    pub status: JobStatus,
    pub retry_count: i32,
    pub created_at: NaiveDateTime,
    pub next_execution_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize)]
pub struct JobsResponse {
    pub stats: Vec<JobTypeStat>,
    pub jobs: Vec<JobSummary>,
}

#[derive(Debug, Serialize)]
pub struct PlansResponse {
    pub plans: Vec<String>,
}

/// One time series for the business-stats sparklines (`stat_snapshot` history).
#[derive(Debug, Serialize)]
pub struct MetricSeriesDto {
    pub metric: String,
    pub dimension: Option<String>,
    pub label: String,
    pub points: Vec<MetricPointDto>,
}

#[derive(Debug, Serialize)]
pub struct MetricPointDto {
    pub captured_at: NaiveDateTime,
    pub value: f64,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub window_days: i64,
    pub series: Vec<MetricSeriesDto>,
}
