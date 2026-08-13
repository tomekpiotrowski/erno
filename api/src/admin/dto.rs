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
    pub last_active_at: Option<NaiveDateTime>,
    pub subscription_type: Option<String>,
    pub subscription_plan: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Serialize)]
pub struct UserListResponse {
    pub users: Vec<UserSummary>,
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
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
    pub oauth_providers: Vec<String>,
    pub subscription_history: Vec<SubscriptionInfo>,
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
pub struct JobExecutionDto {
    pub id: Uuid,
    pub result: String,
    pub started_at: NaiveDateTime,
    pub finished_at: NaiveDateTime,
    pub execution_time_ms: i64,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobDetailResponse {
    pub job: JobSummary,
    pub arguments: serde_json::Value,
    pub executions: Vec<JobExecutionDto>,
}

#[derive(Debug, Serialize)]
pub struct EmailMessageDto {
    pub id: Uuid,
    pub to: String,
    pub from: String,
    pub subject: String,
    pub template: Option<String>,
    pub user_id: Option<Uuid>,
    pub job_id: Option<Uuid>,
    pub status: String,
    pub error: Option<String>,
    pub sent_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Serialize)]
pub struct EmailListResponse {
    pub emails: Vec<EmailMessageDto>,
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
}

#[derive(Debug, Serialize)]
pub struct TableCountDto {
    pub table: String,
    pub approx_rows: i64,
    pub n_dead_tup: i64,
    pub last_analyze: Option<NaiveDateTime>,
    pub approx: bool,
}

#[derive(Debug, Serialize)]
pub struct TablesResponse {
    pub tables: Vec<TableCountDto>,
}

#[derive(Debug, Serialize)]
pub struct AdminEventDto {
    pub id: Uuid,
    pub name: String,
    pub user_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Serialize)]
pub struct EventsResponse {
    pub events: Vec<AdminEventDto>,
}

#[derive(Debug, Serialize)]
pub struct PlansResponse {
    pub plans: Vec<String>,
}
