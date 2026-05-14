use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::action::RateLimitAction;
use super::backend::{InMemoryBackend, RateLimitBackend};

/// A single tier in a multi-tier rate limit.
///
/// Represents a constraint: no more than `max_requests` in a `window_secs` window.
/// Multiple tiers catch attacks at different speeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitTier {
    /// Duration of the rate limit window in seconds
    pub window_secs: u64,
    /// Maximum number of requests allowed in the window
    pub max_requests: u32,
}

/// Configuration for a specific action's rate limit.
///
/// Uses multiple tiers to catch attacks at different speeds:
/// - Tier 1: fast burst detection (e.g., 2 requests in 5 seconds)
/// - Tier 2: moderate rate (e.g., 5 requests per minute)
/// - Tier 3: sustained rate (e.g., 20 requests per hour)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRateLimit {
    /// Multiple rate limit tiers, checked in order.
    /// If any tier is exceeded, the request is rate-limited.
    pub tiers: Vec<RateLimitTier>,
}

/// Global rate limiting configuration.
///
/// Contains default settings and per-action overrides. When an action
/// is not found in the overrides, the default settings are used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Whether rate limiting is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Trust X-Forwarded-For / X-Real-IP headers for client IP.
    /// Enable only when running behind a trusted reverse proxy (nginx, Caddy, etc.).
    /// Without this, all users behind the same proxy share one rate limit quota.
    #[serde(default)]
    pub trust_proxy: bool,

    /// Default time window in seconds
    #[serde(default = "default_window_secs")]
    pub default_window_secs: u64,

    /// Default maximum requests per window
    #[serde(default = "default_max_requests")]
    pub default_max_requests: u32,

    /// Multiplier for exponential backoff (e.g., 2.0 doubles the penalty each time)
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: f64,

    /// Per-action rate limit overrides. Keys are action names (e.g. `"user_create"`).
    #[serde(default)]
    pub actions: HashMap<String, ActionRateLimit>,
}

fn default_enabled() -> bool {
    true
}

fn default_window_secs() -> u64 {
    60
}

fn default_max_requests() -> u32 {
    100
}

fn default_backoff_multiplier() -> f64 {
    2.0
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            trust_proxy: false,
            default_window_secs: default_window_secs(),
            default_max_requests: default_max_requests(),
            backoff_multiplier: default_backoff_multiplier(),
            actions: Self::default_actions(),
        }
    }
}

impl RateLimitConfig {
    /// Pre-configured limits for sensitive auth endpoints.
    ///
    /// These match the action names emitted by the route-tagging middleware in
    /// `router.rs`. Any action not listed here falls back to the default tier.
    fn default_actions() -> HashMap<String, ActionRateLimit> {
        let mut actions = HashMap::new();

        actions.insert(
            "user_create".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier { window_secs: 5, max_requests: 2 },
                    RateLimitTier { window_secs: 60, max_requests: 5 },
                    RateLimitTier { window_secs: 3600, max_requests: 20 },
                ],
            },
        );

        actions.insert(
            "user_verify".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier { window_secs: 5, max_requests: 15 },
                    RateLimitTier { window_secs: 20, max_requests: 30 },
                    RateLimitTier { window_secs: 60, max_requests: 60 },
                    RateLimitTier { window_secs: 300, max_requests: 150 },
                ],
            },
        );

        actions.insert(
            "user_login".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier { window_secs: 5, max_requests: 5 },
                    RateLimitTier { window_secs: 60, max_requests: 10 },
                    RateLimitTier { window_secs: 3600, max_requests: 30 },
                ],
            },
        );

        actions.insert(
            "password_reset_request".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier { window_secs: 5, max_requests: 2 },
                    RateLimitTier { window_secs: 60, max_requests: 5 },
                    RateLimitTier { window_secs: 3600, max_requests: 10 },
                ],
            },
        );

        actions.insert(
            "password_reset_confirm".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier { window_secs: 5, max_requests: 5 },
                    RateLimitTier { window_secs: 60, max_requests: 10 },
                    RateLimitTier { window_secs: 3600, max_requests: 20 },
                ],
            },
        );

        actions.insert(
            "resend_verification".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier { window_secs: 5, max_requests: 2 },
                    RateLimitTier { window_secs: 60, max_requests: 5 },
                    RateLimitTier { window_secs: 3600, max_requests: 10 },
                ],
            },
        );

        actions
    }

    /// Get the rate limit for a specific action.
    ///
    /// Returns the action-specific limit if configured, otherwise generates a
    /// two-tier default from `default_window_secs` / `default_max_requests`.
    pub fn get_limit(&self, action: &RateLimitAction) -> ActionRateLimit {
        self.actions
            .get(action.as_str())
            .cloned()
            .unwrap_or_else(|| {
                // Short burst window (1/12 of the main window) + full window.
                // .max(1) prevents a zero window if default_window_secs < 12.
                ActionRateLimit {
                    tiers: vec![
                        RateLimitTier {
                            window_secs: (self.default_window_secs / 12).max(1),
                            max_requests: (self.default_max_requests / 10).max(1),
                        },
                        RateLimitTier {
                            window_secs: self.default_window_secs,
                            max_requests: self.default_max_requests,
                        },
                    ],
                }
            })
    }
}

/// Rate limiting state — config plus a pluggable storage backend.
///
/// The default constructor uses [`InMemoryBackend`], which is correct for
/// single-replica deployments. For multi-replica use [`RateLimitState::with_backend`]
/// and supply a shared-store backend (Redis, Postgres, etc.).
#[derive(Clone)]
pub struct RateLimitState {
    config: Arc<RateLimitConfig>,
    backend: Arc<dyn RateLimitBackend>,
    /// Kept as a concrete reference so the cleanup task can call
    /// `cleanup_expired_entries` without needing a trait method or downcasting.
    in_memory: Option<Arc<InMemoryBackend>>,
}

impl fmt::Debug for RateLimitState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimitState")
            .field("config", &self.config)
            .field("backend", &"<dyn RateLimitBackend>")
            .finish()
    }
}

impl RateLimitState {
    /// Create a new state with the default in-memory backend.
    pub fn new(config: RateLimitConfig) -> Self {
        let backend = Arc::new(InMemoryBackend::new());
        Self {
            config: Arc::new(config),
            in_memory: Some(backend.clone()),
            backend,
        }
    }

    /// Create a new state with a custom backend (e.g. Redis for multi-replica).
    pub fn with_backend(config: RateLimitConfig, backend: Arc<dyn RateLimitBackend>) -> Self {
        Self {
            config: Arc::new(config),
            backend,
            in_memory: None,
        }
    }

    /// Whether proxy headers (X-Forwarded-For, X-Real-IP) should be trusted for IP extraction.
    pub fn trust_proxy(&self) -> bool {
        self.config.trust_proxy
    }

    /// Check if a request from `ip` for `action` is within the rate limit.
    ///
    /// Returns `Ok(())` if allowed, or `Err(retry_after)` if blocked.
    pub async fn check_rate_limit(&self, ip: IpAddr, action: &RateLimitAction) -> Result<(), Duration> {
        if !self.config.enabled {
            return Ok(());
        }
        let limit = self.config.get_limit(action);
        let key = format!("{}/{}", ip, action.as_str());
        self.backend.check_rate_limit(&key, &limit, self.config.backoff_multiplier).await
    }

    /// Remove stale in-memory entries. No-op for non-in-memory backends.
    ///
    /// Call periodically (e.g. every 5 minutes) to prevent unbounded memory growth.
    pub fn cleanup_expired_entries(&self) {
        if let Some(mem) = &self.in_memory {
            mem.cleanup_expired_entries();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(enabled: bool, actions: HashMap<String, ActionRateLimit>, default_max: u32) -> RateLimitState {
        RateLimitState::new(RateLimitConfig {
            enabled,
            trust_proxy: false,
            default_window_secs: 60,
            default_max_requests: default_max,
            backoff_multiplier: 2.0,
            actions,
        })
    }

    fn action_limit(window_secs: u64, max_requests: u32) -> ActionRateLimit {
        ActionRateLimit { tiers: vec![RateLimitTier { window_secs, max_requests }] }
    }

    #[tokio::test]
    async fn test_single_tier_allows_requests_under_limit() {
        let mut actions = HashMap::new();
        actions.insert("test".to_string(), action_limit(60, 5));
        let state = make_state(true, actions, 5);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");
        for _ in 0..5 {
            assert!(state.check_rate_limit(ip, &action).await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_single_tier_blocks_excess_requests() {
        let mut actions = HashMap::new();
        actions.insert("test".to_string(), action_limit(60, 3));
        let state = make_state(true, actions, 10);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");
        for _ in 0..3 {
            assert!(state.check_rate_limit(ip, &action).await.is_ok());
        }
        assert!(state.check_rate_limit(ip, &action).await.is_err());
    }

    #[tokio::test]
    async fn test_multi_tier_catches_fast_burst() {
        let mut actions = HashMap::new();
        actions.insert("test".to_string(), ActionRateLimit {
            tiers: vec![
                RateLimitTier { window_secs: 5, max_requests: 2 },
                RateLimitTier { window_secs: 60, max_requests: 100 },
            ],
        });
        let state = make_state(true, actions, 100);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");
        assert!(state.check_rate_limit(ip, &action).await.is_ok());
        assert!(state.check_rate_limit(ip, &action).await.is_ok());
        assert!(state.check_rate_limit(ip, &action).await.is_err());
    }

    #[tokio::test]
    async fn test_multi_tier_allows_normal_rate() {
        let mut actions = HashMap::new();
        actions.insert("test".to_string(), ActionRateLimit {
            tiers: vec![
                RateLimitTier { window_secs: 5, max_requests: 100 },
                RateLimitTier { window_secs: 60, max_requests: 200 },
            ],
        });
        let state = make_state(true, actions, 100);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");
        for _ in 0..50 {
            assert!(state.check_rate_limit(ip, &action).await.is_ok(), "Request should succeed");
        }
    }

    #[tokio::test]
    async fn test_disabled_rate_limiting() {
        let state = make_state(false, HashMap::new(), 1);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");
        for _ in 0..100 {
            assert!(state.check_rate_limit(ip, &action).await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_action_specific_limits() {
        let mut actions = HashMap::new();
        actions.insert("strict".to_string(), action_limit(60, 2));
        let state = make_state(true, actions, 100);
        let ip = "127.0.0.1".parse().unwrap();
        let strict = RateLimitAction::new("strict");
        let normal = RateLimitAction::new("normal");

        assert!(state.check_rate_limit(ip, &strict).await.is_ok());
        assert!(state.check_rate_limit(ip, &strict).await.is_ok());
        assert!(state.check_rate_limit(ip, &strict).await.is_err());

        let ip2 = "127.0.0.2".parse().unwrap();
        for _ in 0..10 {
            assert!(state.check_rate_limit(ip2, &normal).await.is_ok());
        }
        assert!(state.check_rate_limit(ip2, &normal).await.is_err());
    }

    #[tokio::test]
    async fn test_custom_backend_accepted() {
        use async_trait::async_trait;

        struct AlwaysAllow;

        #[async_trait]
        impl RateLimitBackend for AlwaysAllow {
            async fn check_rate_limit(&self, _key: &str, _limit: &ActionRateLimit, _backoff: f64) -> Result<(), Duration> {
                Ok(())
            }
        }

        let state = RateLimitState::with_backend(
            RateLimitConfig { enabled: true, default_max_requests: 1, ..Default::default() },
            Arc::new(AlwaysAllow),
        );
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");
        // AlwaysAllow never blocks, even past the config limit
        for _ in 0..200 {
            assert!(state.check_rate_limit(ip, &action).await.is_ok());
        }
    }
}
