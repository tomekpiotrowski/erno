use reqwest::{header, Client, StatusCode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AdminClient {
    http: Client,
    base_url: String,
    auth_header: String,
}

#[derive(Debug)]
pub enum ClientError {
    Http(reqwest::Error),
    Api { status: StatusCode, message: String },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {e}"),
            Self::Api { status, message } => write!(f, "API error ({status}): {message}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl AdminClient {
    pub fn new(base_url: &str, username: &str, password: &str) -> Result<Self, ClientError> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("{username}:{password}"),
        );
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            base_url,
            auth_header: format!("Basic {encoded}"),
        })
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, ClientError> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header(header::AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        Self::parse(resp).await
    }

    async fn post_empty(&self, path: &str) -> Result<(), ClientError> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header(header::AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let message = resp.text().await.unwrap_or_default();
            Err(ClientError::Api { status, message })
        }
    }

    async fn post_json_body<B: Serialize, T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header(header::AUTHORIZATION, &self.auth_header)
            .json(body)
            .send()
            .await?;
        Self::parse(resp).await
    }

    async fn post_json_empty<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
    ) -> Result<T, ClientError> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header(header::AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        Self::parse(resp).await
    }

    async fn delete(&self, path: &str) -> Result<(), ClientError> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .http
            .delete(&url)
            .header(header::AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let message = resp.text().await.unwrap_or_default();
            Err(ClientError::Api { status, message })
        }
    }

    async fn parse<T: for<'de> Deserialize<'de>>(
        resp: reqwest::Response,
    ) -> Result<T, ClientError> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp.json().await?)
        } else {
            let message = resp.text().await.unwrap_or_default();
            Err(ClientError::Api { status, message })
        }
    }

    pub async fn dashboard(&self) -> Result<DashboardResponse, ClientError> {
        self.get("/admin/api/dashboard").await
    }

    pub async fn list_users(&self, q: &str) -> Result<UserListResponse, ClientError> {
        let path = if q.is_empty() {
            "/admin/api/users".to_string()
        } else {
            format!(
                "/admin/api/users?q={}",
                urlencoding_lite(q)
            )
        };
        self.get(&path).await
    }

    pub async fn get_user(&self, id: Uuid) -> Result<UserDetailResponse, ClientError> {
        self.get(&format!("/admin/api/users/{id}")).await
    }

    pub async fn activate_user(&self, id: Uuid) -> Result<UserDetailResponse, ClientError> {
        self.post_json_empty(&format!("/admin/api/users/{id}/activate"))
            .await
    }

    pub async fn delete_user(&self, id: Uuid) -> Result<(), ClientError> {
        self.delete(&format!("/admin/api/users/{id}")).await
    }

    pub async fn gift_user(
        &self,
        id: Uuid,
        plan: &str,
        duration_days: u32,
    ) -> Result<UserDetailResponse, ClientError> {
        self.post_json_body(
            &format!("/admin/api/users/{id}/gift"),
            &GiftRequest {
                plan: plan.to_string(),
                duration_days,
            },
        )
        .await
    }

    pub async fn list_jobs(
        &self,
        status: Option<&str>,
        job_type: Option<&str>,
    ) -> Result<JobsResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(s) = status {
            params.push(format!("status={s}"));
        }
        if let Some(t) = job_type {
            params.push(format!("type={}", urlencoding_lite(t)));
        }
        let path = if params.is_empty() {
            "/admin/api/jobs".to_string()
        } else {
            format!("/admin/api/jobs?{}", params.join("&"))
        };
        self.get(&path).await
    }

    pub async fn retry_job(&self, id: Uuid) -> Result<(), ClientError> {
        self.post_empty(&format!("/admin/api/jobs/{id}/retry")).await
    }

    pub async fn plans(&self) -> Result<PlansResponse, ClientError> {
        self.get("/admin/api/plans").await
    }

    pub async fn stats(&self, days: i64) -> Result<StatsResponse, ClientError> {
        self.get(&format!("/admin/api/stats?days={days}")).await
    }
}

/// Minimal query encoding (enough for email search).
fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'@' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── DTOs (mirror api/src/admin/dto.rs) ───────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
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
    pub refreshed_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmailJobStat {
    pub name: String,
    pub total: i64,
    pub completed: i64,
    pub failed: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserSummary {
    pub id: Uuid,
    pub email: String,
    pub email_verified_at: Option<chrono::NaiveDateTime>,
    pub subscription_type: Option<String>,
    pub subscription_plan: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserListResponse {
    pub users: Vec<UserSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionInfo {
    pub sub_type: String,
    pub plan: String,
    pub status: String,
    pub expiry: String,
    pub stripe_customer_id: Option<String>,
    pub stripe_sub_id: Option<String>,
    pub cancel_at_period_end: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserDetailResponse {
    pub user: UserSummary,
    pub subscription: Option<SubscriptionInfo>,
}

#[derive(Debug, Serialize)]
struct GiftRequest {
    plan: String,
    duration_days: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JobTypeStat {
    pub job_type: String,
    pub pending: i64,
    pub running: i64,
    pub failed: i64,
    pub completed: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct JobSummary {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub retry_count: i32,
    pub created_at: chrono::NaiveDateTime,
    pub next_execution_at: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JobsResponse {
    pub stats: Vec<JobTypeStat>,
    pub jobs: Vec<JobSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlansResponse {
    pub plans: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct MetricPointDto {
    pub captured_at: chrono::NaiveDateTime,
    pub value: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct MetricSeriesDto {
    pub metric: String,
    pub dimension: Option<String>,
    pub label: String,
    pub points: Vec<MetricPointDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatsResponse {
    pub window_days: i64,
    pub series: Vec<MetricSeriesDto>,
}
