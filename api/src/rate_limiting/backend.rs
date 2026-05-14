use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use tracing::{trace, warn};

use super::rate_limit_state::ActionRateLimit;

/// Pluggable storage backend for rate limiting.
///
/// The default [`InMemoryBackend`] works correctly for single-replica deployments.
/// For multi-replica setups, implement this trait against a shared store such as
/// Redis or PostgreSQL and pass it to [`RateLimitState::with_backend`].
///
/// # Example (Redis backend sketch)
///
/// ```rust,ignore
/// struct RedisBackend { pool: deadpool_redis::Pool }
///
/// #[async_trait]
/// impl RateLimitBackend for RedisBackend {
///     async fn check_rate_limit(&self, key: &str, limit: &ActionRateLimit, backoff_multiplier: f64) -> Result<(), Duration> {
///         // sliding window via Redis ZADD + ZREMRANGEBYSCORE + ZCARD in a Lua script
///         todo!()
///     }
/// }
/// ```
#[async_trait]
pub trait RateLimitBackend: Send + Sync {
    /// Check whether the request identified by `key` is within limits.
    ///
    /// `key` is a composite string: `"{ip}/{action}"` (e.g. `"1.2.3.4/user_create"`).
    /// Returns `Ok(())` if the request is allowed, or `Err(retry_after)` if it should
    /// be rejected. The `retry_after` duration is suitable for the `Retry-After` header.
    async fn check_rate_limit(
        &self,
        key: &str,
        limit: &ActionRateLimit,
        backoff_multiplier: f64,
    ) -> Result<(), Duration>;
}

/// Per-client sliding-window state tracked by [`InMemoryBackend`].
#[derive(Debug, Clone)]
pub(super) struct ClientState {
    requests: Vec<Instant>,
    violations: u32,
    blocked_until: Option<Instant>,
}

impl ClientState {
    pub(super) fn new() -> Self {
        Self {
            requests: Vec::new(),
            violations: 0,
            blocked_until: None,
        }
    }

    pub(super) fn is_blocked(&self) -> Option<Duration> {
        if let Some(blocked_until) = self.blocked_until {
            let now = Instant::now();
            if now < blocked_until {
                return Some(blocked_until - now);
            }
        }
        None
    }

    fn cleanup_expired(&mut self, window: Duration) {
        let cutoff = Instant::now() - window;
        self.requests.retain(|&t| t > cutoff);
    }

    pub(super) fn record_request(
        &mut self,
        limit: &ActionRateLimit,
        backoff_multiplier: f64,
    ) -> Option<Duration> {
        let now = Instant::now();

        // Reset violations once the block has fully expired so past incidents don't
        // cause unbounded penalty escalation for legitimate users.
        if self.blocked_until.is_some_and(|t| now >= t) {
            self.violations = 0;
            self.blocked_until = None;
        }

        let max_window = limit
            .tiers
            .iter()
            .map(|t| Duration::from_secs(t.window_secs))
            .max()
            .unwrap_or(Duration::from_secs(60));

        self.cleanup_expired(max_window);

        for tier in &limit.tiers {
            let window = Duration::from_secs(tier.window_secs);
            let cutoff = now - window;
            let count = self.requests.iter().filter(|&&t| t > cutoff).count();

            if count >= tier.max_requests as usize {
                self.violations += 1;
                let base = Duration::from_secs(tier.window_secs);
                let penalty = base.mul_f64(backoff_multiplier.powi(self.violations as i32 - 1));
                self.blocked_until = Some(now + penalty);

                warn!(
                    tier_window_secs = tier.window_secs,
                    tier_max_requests = tier.max_requests,
                    violations = self.violations,
                    penalty_secs = penalty.as_secs(),
                    "Rate limit tier exceeded with exponential backoff"
                );

                return Some(penalty);
            }
        }

        self.requests.push(now);
        trace!(total_requests = self.requests.len(), "Request recorded within all rate limit tiers");
        None
    }
}

/// In-memory rate limiting backend.
///
/// Uses a [`DashMap`] for concurrent access across Tokio tasks. All state is
/// local to the process — use a shared-store backend for multi-replica deployments.
#[derive(Debug)]
pub struct InMemoryBackend {
    clients: Arc<DashMap<String, ClientState>>,
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(DashMap::new()),
        }
    }

    /// Remove entries for clients that haven't made a request in the last hour
    /// and are no longer blocked. Call this periodically to bound memory usage.
    pub fn cleanup_expired_entries(&self) {
        let cutoff = Instant::now() - Duration::from_secs(3600);
        self.clients.retain(|_key, client| {
            if let Some(blocked_until) = client.blocked_until {
                if Instant::now() < blocked_until {
                    return true;
                }
            }
            !client.requests.is_empty() && client.requests.last().is_some_and(|&t| t > cutoff)
        });
    }
}

#[async_trait]
impl RateLimitBackend for InMemoryBackend {
    async fn check_rate_limit(
        &self,
        key: &str,
        limit: &ActionRateLimit,
        backoff_multiplier: f64,
    ) -> Result<(), Duration> {
        let mut entry = self.clients.entry(key.to_string()).or_insert_with(ClientState::new);
        let client = entry.value_mut();

        if let Some(remaining) = client.is_blocked() {
            return Err(remaining);
        }

        if let Some(penalty) = client.record_request(limit, backoff_multiplier) {
            return Err(penalty);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rate_limiting::rate_limit_state::{ActionRateLimit, RateLimitTier};

    fn make_limit(window_secs: u64, max_requests: u32) -> ActionRateLimit {
        ActionRateLimit {
            tiers: vec![RateLimitTier { window_secs, max_requests }],
        }
    }

    fn make_multi_tier(tiers: Vec<(u64, u32)>) -> ActionRateLimit {
        ActionRateLimit {
            tiers: tiers.into_iter().map(|(w, m)| RateLimitTier { window_secs: w, max_requests: m }).collect(),
        }
    }

    #[tokio::test]
    async fn test_single_tier_allows_requests_under_limit() {
        let backend = InMemoryBackend::new();
        let limit = make_limit(60, 5);
        for _ in 0..5 {
            assert!(backend.check_rate_limit("ip/action", &limit, 2.0).await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_single_tier_blocks_excess_requests() {
        let backend = InMemoryBackend::new();
        let limit = make_limit(60, 3);
        for _ in 0..3 {
            assert!(backend.check_rate_limit("ip/action", &limit, 2.0).await.is_ok());
        }
        assert!(backend.check_rate_limit("ip/action", &limit, 2.0).await.is_err());
    }

    #[tokio::test]
    async fn test_multi_tier_catches_fast_burst() {
        let backend = InMemoryBackend::new();
        let limit = make_multi_tier(vec![(5, 2), (60, 100)]);
        assert!(backend.check_rate_limit("ip/action", &limit, 2.0).await.is_ok());
        assert!(backend.check_rate_limit("ip/action", &limit, 2.0).await.is_ok());
        assert!(backend.check_rate_limit("ip/action", &limit, 2.0).await.is_err());
    }

    #[tokio::test]
    async fn test_multi_tier_allows_normal_rate() {
        let backend = InMemoryBackend::new();
        let limit = make_multi_tier(vec![(5, 100), (60, 200)]);
        for _ in 0..50 {
            assert!(backend.check_rate_limit("ip/action", &limit, 2.0).await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_violations_reset_after_block_expires() {
        use std::thread;

        let backend = InMemoryBackend::new();
        let limit = make_limit(1, 2); // 1s window, max 2

        // Hit the limit → violations = 1, penalty = 1s
        assert!(backend.check_rate_limit("ip/test", &limit, 2.0).await.is_ok());
        assert!(backend.check_rate_limit("ip/test", &limit, 2.0).await.is_ok());
        assert!(backend.check_rate_limit("ip/test", &limit, 2.0).await.is_err());

        // Wait for block to expire
        thread::sleep(Duration::from_millis(1100));

        // First request after expiry should succeed and reset violations
        assert!(backend.check_rate_limit("ip/test", &limit, 2.0).await.is_ok(), "First request after block should succeed");

        // Hit the limit again — penalty should be back to 1s (violations reset to 0)
        assert!(backend.check_rate_limit("ip/test", &limit, 2.0).await.is_ok());
        let err = backend.check_rate_limit("ip/test", &limit, 2.0).await;
        assert!(err.is_err());
        assert!(err.unwrap_err().as_secs() <= 1, "Penalty should be base window, not doubled");
    }
}
