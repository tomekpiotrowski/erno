use lettre::message::Mailbox;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    #[default]
    Local,
    S3,
}

/// Works with AWS S3 and any S3-compatible service (Digital Ocean Spaces, MinIO, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Custom endpoint URL for S3-compatible services, e.g. <https://nyc3.digitaloceanspaces.com>
    pub endpoint: Option<String>,
    /// Optional CDN URL prefix; if set, used instead of presigned URLs
    pub cdn_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    #[serde(default)]
    pub backend: StorageBackend,
    /// Root path for local storage (default: "./storage")
    pub local_path: Option<String>,
    pub s3: Option<S3Config>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StripeConfig {
    pub secret_key: String,
    pub webhook_secret: String,
    pub admin_token: String,
    /// Maps plan name to Stripe Price ID, e.g. {"pro": "price_xxx"}
    #[serde(default)]
    pub price_ids: HashMap<String, String>,
    pub success_url: String,
    pub cancel_url: String,
    pub portal_return_url: String,
}

pub use crate::error_reporting::config::ErrorReportingConfig;
pub use crate::rate_limiting::rate_limit_state::RateLimitConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    #[serde(default = "default_metrics_enabled")]
    pub enabled: bool,
    #[serde(default = "default_metrics_path")]
    pub path: String,
    pub auth_token: Option<String>,
    #[serde(default = "default_db_stats_interval")]
    pub db_stats_interval_seconds: u64,
    #[serde(default)]
    pub table_counts: Vec<String>,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: default_metrics_enabled(),
            path: default_metrics_path(),
            auth_token: None,
            db_stats_interval_seconds: default_db_stats_interval(),
            table_counts: Vec::new(),
        }
    }
}

const fn default_metrics_enabled() -> bool {
    true
}

fn default_metrics_path() -> String {
    "/metrics".to_string()
}

const fn default_db_stats_interval() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config<ExtraConfig = ()> {
    pub tracing: TracingConfig,
    pub database: DatabaseConfig,
    pub jobs: JobsConfig,
    pub server: ServerConfig,
    pub email: EmailConfig,
    /// Optional directory of branded HTML email templates
    /// (`verification.html`, `password_reset.html`, `already_registered.html`).
    /// Placeholders: `{{verify_url}}`, `{{reset_url}}`, `{{login_url}}`,
    /// `{{email}}`, `{{app_name}}`, `{{expiry_hours}}`.
    #[serde(default)]
    pub email_templates_dir: Option<String>,
    /// API server base URL (used for CORS, self-referencing API links).
    pub api_url: String,
    /// Frontend app URL used in email links (verify-email, password-reset, etc.).
    /// Defaults to `api_url` when not set.
    #[serde(default)]
    pub app_url: Option<String>,
    pub auth: AuthConfig,
    pub rate_limiting: RateLimitConfig,
    pub stripe: Option<StripeConfig>,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub cors: CorsConfig,
    /// Days to keep `email_messages` rows (default 30). Purged by the job-cleanup loop.
    #[serde(default = "default_email_log_retention_days")]
    pub email_log_retention_days: u64,
    /// Operator admin API (`/admin/api/*`). Disabled when absent or when
    /// `password_hash` is empty.
    #[serde(default)]
    pub admin: Option<AdminConfig>,
    /// Reporting this app's own errors to a monitoring collector.
    /// Inert until `collector_url` is set.
    #[serde(default)]
    pub error_reporting: ErrorReportingConfig,
    #[serde(flatten, default)]
    pub extra: ExtraConfig,
}

/// Configuration for the HTTP admin API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    /// Basic-auth username. Defaults to `"admin"`.
    #[serde(default = "default_admin_username")]
    pub username: String,
    /// Argon2 PHC password hash. Required to enable the admin API.
    pub password_hash: String,
}

impl AdminConfig {
    /// Whether the admin API should be mounted.
    pub fn is_enabled(&self) -> bool {
        !self.password_hash.is_empty()
    }
}

fn default_admin_username() -> String {
    "admin".to_string()
}

impl<ExtraConfig> Config<ExtraConfig> {
    /// Returns the frontend app URL for use in email links.
    pub fn app_url(&self) -> &str {
        self.app_url.as_deref().unwrap_or(&self.api_url)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub secret: String,
    /// Access token TTL in minutes. Default: 15.
    #[serde(default = "default_access_token_minutes")]
    pub access_token_minutes: u64,
    pub one_time_token_expiry_hours: u64,
    /// Refresh token TTL in days. Default: 30.
    #[serde(default = "default_refresh_token_days")]
    pub refresh_token_days: u64,
    /// Social login providers. Absent or incomplete entries are treated as disabled.
    #[serde(default)]
    pub oauth: OauthConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OauthConfig {
    pub google: Option<OauthClientConfig>,
    pub discord: Option<OauthClientConfig>,
    pub apple: Option<AppleOauthConfig>,
}

/// Google / Discord OAuth2 client credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OauthClientConfig {
    pub client_id: String,
    pub client_secret: String,
}

/// Sign in with Apple — Services ID + key used to mint the client secret JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppleOauthConfig {
    /// Services ID (client_id) for web OAuth.
    pub client_id: String,
    pub team_id: String,
    pub key_id: String,
    /// Contents of the `.p8` private key (PEM), including headers.
    pub private_key_pem: String,
}

const fn default_access_token_minutes() -> u64 {
    15
}

const fn default_refresh_token_days() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum EmailConfig {
    /// Mock mailer that captures emails for testing
    Mock,
    /// Real SMTP configuration for sending emails
    Smtp {
        host: String,
        port: u16,
        #[serde(deserialize_with = "deserialize_mailbox")]
        sender: Mailbox,
        username: Option<String>,
        password: Option<String>,
        #[serde(default = "default_use_tls")]
        use_tls: bool,
    },
}

fn deserialize_mailbox<'de, D>(deserializer: D) -> Result<Mailbox, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

fn default_use_tls() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CorsConfig {
    /// Origins allowed to call the API, e.g. ["http://localhost:4200"].
    /// An empty list disables CORS headers entirely.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TracingConfig {
    pub log_level: String,
    /// OpenTelemetry export. Inert until `otel.endpoint` is set, so a fresh
    /// app never tries to push traces or logs.
    #[serde(default)]
    pub otel: OtelConfig,
}

/// Where to send traces and logs, and how much of each.
///
/// Empty `endpoint` disables trace export. Empty `logs_endpoint` falls back to
/// `endpoint` (Tempo and Loki share a public `/otlp` base in production; they
/// listen on different ports in development).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelConfig {
    /// OTLP/HTTP traces base URL, e.g. `http://127.0.0.1:4318`. The exporter
    /// appends `/v1/traces`. Empty disables trace export.
    #[serde(default)]
    pub endpoint: String,
    /// OTLP/HTTP logs base URL, e.g. `http://127.0.0.1:3100/otlp`. Empty
    /// inherits `endpoint`. The exporter appends `/v1/logs`.
    #[serde(default)]
    pub logs_endpoint: String,
    /// Bearer token sent as `Authorization`. Production uses the trusted
    /// server ingest token; development leaves this empty.
    #[serde(default)]
    pub token: String,
    /// Head-sampling ratio for traces. `1.0` keeps everything, `0.0` drops
    /// everything. Parent-based: a sampled parent stays sampled.
    #[serde(default = "default_otel_sample_ratio")]
    pub sample_ratio: f64,
    /// `service.name` resource attribute. Empty becomes `erno`.
    #[serde(default)]
    pub service_name: String,
    /// Minimum severity exported as OTEL logs (stdout is `[tracing] log_level`).
    /// Empty disables log export.
    #[serde(default)]
    pub log_level: String,
    /// OTLP/HTTP metrics base URL. Empty inherits `endpoint`. The pusher
    /// appends `/v1/metrics`. Metrics are pushed, never scraped: `/metrics`
    /// still answers locally, but nothing needs to reach in for it.
    #[serde(default)]
    pub metrics_endpoint: String,
    /// Seconds between metric pushes. `0` disables the pusher.
    #[serde(default = "default_otel_metrics_interval")]
    pub metrics_interval_seconds: u64,
    /// Tempo/Loki tenant, sent as `X-Scope-OrgID`.
    ///
    /// Only for a process that pushes to a multi-tenant store *directly*.
    /// Applications go through the collector's nginx, which sets the tenant
    /// from their ingest token — they must leave this empty, or they would be
    /// naming a tenant for themselves. The collector's own in-cluster push does
    /// not pass through nginx, which is what this exists for.
    #[serde(default)]
    pub tenant: String,
}

impl OtelConfig {
    /// Whether traces should be exported.
    #[must_use]
    pub fn traces_enabled(&self) -> bool {
        !self.endpoint.trim().is_empty()
    }

    /// OTLP/HTTP metrics base URL, if metric push is on.
    #[must_use]
    pub fn metrics_target(&self) -> Option<&str> {
        if self.metrics_interval_seconds == 0 {
            return None;
        }
        let target = if self.metrics_endpoint.trim().is_empty() {
            self.endpoint.trim()
        } else {
            self.metrics_endpoint.trim()
        };
        (!target.is_empty()).then_some(target)
    }

    /// OTLP/HTTP logs base URL, if log export is on.
    #[must_use]
    pub fn logs_target(&self) -> Option<&str> {
        if self.log_level.trim().is_empty() {
            return None;
        }
        let target = if self.logs_endpoint.trim().is_empty() {
            self.endpoint.trim()
        } else {
            self.logs_endpoint.trim()
        };
        if target.is_empty() {
            None
        } else {
            Some(target)
        }
    }
}

const fn default_otel_sample_ratio() -> f64 {
    0.1
}

const fn default_otel_metrics_interval() -> u64 {
    15
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            logs_endpoint: String::new(),
            token: String::new(),
            sample_ratio: default_otel_sample_ratio(),
            service_name: String::new(),
            log_level: String::new(),
            metrics_endpoint: String::new(),
            metrics_interval_seconds: default_otel_metrics_interval(),
            tenant: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub pool_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobsConfig {
    /// App-wide default retry/failure settings. Worker pools and individual
    /// jobs may override any of these; anything left unset inherits these values.
    #[serde(default)]
    pub defaults: JobRetryDefaults,
    pub cleanup: CleanupConfig,
    pub workers: WorkersConfig,
}

/// App-wide defaults for job execution timeout and retry behaviour.
///
/// Resolution precedence for any single setting is: per-job override →
/// worker-pool override → these app-wide defaults.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct JobRetryDefaults {
    /// Job execution timeout in seconds (default: 300)
    #[serde(default = "default_job_timeout")]
    pub job_timeout: u32,
    /// Maximum number of retry attempts for failed jobs (default: 4)
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    /// Base delay in seconds before first retry (default: 60)
    #[serde(default = "default_base_retry_delay")]
    pub base_retry_delay_seconds: u64,
    /// Exponential backoff multiplier (default: 5)
    #[serde(default = "default_retry_multiplier")]
    pub retry_backoff_multiplier: u64,
}

impl Default for JobRetryDefaults {
    fn default() -> Self {
        Self {
            job_timeout: default_job_timeout(),
            max_retries: default_max_retries(),
            base_retry_delay_seconds: default_base_retry_delay(),
            retry_backoff_multiplier: default_retry_multiplier(),
        }
    }
}

/// Fully resolved retry settings for a single job execution, after merging
/// per-job overrides, worker-pool overrides, and app-wide defaults.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedRetryConfig {
    pub job_timeout: u32,
    pub max_retries: i32,
    pub base_retry_delay_seconds: u64,
    pub retry_backoff_multiplier: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupConfig {
    /// Interval between cleanup runs in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_cleanup_interval")]
    pub interval_seconds: u64,
    /// Retention period for completed jobs in seconds (default: 7200 = 2 hours)
    #[serde(default = "default_completed_retention")]
    pub completed_retention_seconds: u64,
    /// Retention period for failed jobs in seconds (default: 172800 = 2 days)
    #[serde(default = "default_failed_retention")]
    pub failed_retention_seconds: u64,
    /// Maximum number of jobs to delete in a single batch (default: 1000)
    #[serde(default = "default_cleanup_batch_size")]
    pub batch_size: usize,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            interval_seconds: default_cleanup_interval(),
            completed_retention_seconds: default_completed_retention(),
            failed_retention_seconds: default_failed_retention(),
            batch_size: default_cleanup_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkersConfig {
    #[serde(flatten)]
    pub workers: HashMap<String, WorkerQueueConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerQueueConfig {
    pub jobs: Vec<String>,
    pub count: u32,
    /// Job execution timeout in seconds. Unset inherits `jobs.defaults.job_timeout`.
    #[serde(default)]
    pub job_timeout: Option<u32>,
    /// Maximum retry attempts for failed jobs. Unset inherits `jobs.defaults.max_retries`.
    #[serde(default)]
    pub max_retries: Option<i32>,
    /// Base delay before first retry. Unset inherits `jobs.defaults.base_retry_delay_seconds`.
    #[serde(default)]
    pub base_retry_delay_seconds: Option<u64>,
    /// Backoff multiplier. Unset inherits `jobs.defaults.retry_backoff_multiplier`.
    #[serde(default)]
    pub retry_backoff_multiplier: Option<u64>,
}

const fn default_max_retries() -> i32 {
    4
}

const fn default_job_timeout() -> u32 {
    300 // 5 minutes
}

const fn default_base_retry_delay() -> u64 {
    60
}

const fn default_retry_multiplier() -> u64 {
    5
}

const fn default_cleanup_interval() -> u64 {
    3600 // 1 hour
}

const fn default_completed_retention() -> u64 {
    7200 // 2 hours
}

const fn default_failed_retention() -> u64 {
    172_800 // 2 days
}

const fn default_cleanup_batch_size() -> usize {
    1000
}

const fn default_email_log_retention_days() -> u64 {
    30
}
