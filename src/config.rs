use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

use lettre::message::Mailbox;

pub use crate::rate_limiting::rate_limit_state::RateLimitConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub tracing: TracingConfig,
    pub database: DatabaseConfig,
    pub jobs: JobsConfig,
    pub server: ServerConfig,
    pub email: EmailConfig,
    pub base_url: String,
    pub jwt: JwtConfig,
    pub password_reset: PasswordResetConfig,
    pub rate_limiting: RateLimitConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    pub secret: String,
    pub expiration_days: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordResetConfig {
    pub token_expiration_hours: u64,
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
pub struct TracingConfig {
    pub log_level: String,
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
    pub cleanup: CleanupConfig,
    pub workers: WorkersConfig,
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
    /// Job execution timeout in seconds (default: 300)
    #[serde(default = "default_job_timeout")]
    pub job_timeout: u32,
    /// Maximum number of retry attempts for failed jobs (default: 4)
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    /// Base delay in seconds before first retry (default: 60)
    #[serde(default = "default_base_retry_delay")]
    pub base_retry_delay_seconds: u64,
    /// Exponential backoff multiplier (default: 5.0)
    #[serde(default = "default_retry_multiplier")]
    pub retry_backoff_multiplier: u64,
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
