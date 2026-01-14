use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};

use super::action::RateLimitAction;

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
/// - Tier 1: fast burst detection (e.g., 20 requests in 5 seconds)
/// - Tier 2: moderate rate (e.g., 30 requests in 10 seconds)
/// - Tier 3: sustained rate (e.g., 100 requests in 60 seconds)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRateLimit {
    /// Multiple rate limit tiers, checked in order
    /// If any tier is exceeded, the request is rate-limited
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

    /// Default time window in seconds
    #[serde(default = "default_window_secs")]
    pub default_window_secs: u64,

    /// Default maximum requests per window
    #[serde(default = "default_max_requests")]
    pub default_max_requests: u32,

    /// Multiplier for exponential backoff (e.g., 2.0 doubles the penalty each time)
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: f64,

    /// Per-action rate limit overrides
    #[serde(default)]
    pub actions: HashMap<String, ActionRateLimit>,
}

fn default_enabled() -> bool {
    true
}

fn default_window_secs() -> u64 {
    60 // 1 minute
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
            default_window_secs: default_window_secs(),
            default_max_requests: default_max_requests(),
            backoff_multiplier: default_backoff_multiplier(),
            actions: Self::default_actions(),
        }
    }
}

impl RateLimitConfig {
    /// Returns default action rate limits with sensible multi-tier defaults.
    ///
    /// These are designed to catch attacks at different speeds while
    /// allowing normal usage patterns.
    fn default_actions() -> HashMap<String, ActionRateLimit> {
        let mut actions = HashMap::new();

        // User registration - strict multi-tier limit
        // Catches rapid registration attempts while allowing normal signups
        actions.insert(
            "user_create".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier {
                        window_secs: 5,
                        max_requests: 2,
                    }, // Catch immediate attacks
                    RateLimitTier {
                        window_secs: 60,
                        max_requests: 5,
                    }, // Per minute
                    RateLimitTier {
                        window_secs: 3600,
                        max_requests: 20,
                    }, // Per hour
                ],
            },
        );

        // Email verification - moderate multi-tier limit
        actions.insert(
            "user_verify".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier {
                        window_secs: 5,
                        max_requests: 15,
                    }, // Quick verification attempts
                    RateLimitTier {
                        window_secs: 20,
                        max_requests: 30,
                    }, // Per minute
                    RateLimitTier {
                        window_secs: 60,
                        max_requests: 60,
                    }, // Per minute
                    RateLimitTier {
                        window_secs: 300,
                        max_requests: 150,
                    }, // Per 5 minutes
                ],
            },
        );

        actions
    }

    /// Get the rate limit for a specific action.
    ///
    /// Returns the action-specific limit if configured, otherwise
    /// returns default multi-tier limits based on the config's single-tier defaults.
    pub fn get_limit(&self, action: &RateLimitAction) -> ActionRateLimit {
        self.actions
            .get(action.as_str())
            .cloned()
            .unwrap_or_else(|| {
                // Generate multi-tier defaults from single-tier config
                ActionRateLimit {
                    tiers: vec![
                        RateLimitTier {
                            window_secs: self.default_window_secs / 12,
                            max_requests: self.default_max_requests / 10,
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

/// Tracks request history for a specific client.
///
/// Uses sliding windows to count requests in different time buckets,
/// supporting multi-tier rate limiting. Implements exponential backoff
/// for repeated violations across any tier.
#[derive(Debug, Clone)]
struct ClientState {
    /// Timestamps of all requests (used for all windows)
    requests: Vec<Instant>,
    /// Number of times this client has violated rate limits
    violations: u32,
    /// When the client can make requests again (if currently blocked)
    blocked_until: Option<Instant>,
}

impl ClientState {
    fn new() -> Self {
        Self {
            requests: Vec::new(),
            violations: 0,
            blocked_until: None,
        }
    }

    /// Check if this client is currently blocked.
    ///
    /// Returns the remaining block duration if blocked, None otherwise.
    fn is_blocked(&self) -> Option<Duration> {
        if let Some(blocked_until) = self.blocked_until {
            let now = Instant::now();
            if now < blocked_until {
                return Some(blocked_until - now);
            }
        }
        None
    }

    /// Remove expired requests from the sliding window.
    ///
    /// Cleans up request timestamps that fall outside the current
    /// rate limit window to keep memory usage bounded.
    fn cleanup_expired(&mut self, window: Duration) {
        let cutoff = Instant::now() - window;
        self.requests.retain(|&timestamp| timestamp > cutoff);
    }

    /// Record a new request and check if rate limit is exceeded.
    ///
    /// Checks all tiers in the rate limit. If any tier is exceeded,
    /// returns Some(Duration) with the retry-after duration based on
    /// exponential backoff. Otherwise, records the request and returns None.
    fn record_request(
        &mut self,
        limit: &ActionRateLimit,
        backoff_multiplier: f64,
    ) -> Option<Duration> {
        let now = Instant::now();

        // Find the longest window to know how far back we need to keep timestamps
        let max_window = limit
            .tiers
            .iter()
            .map(|t| Duration::from_secs(t.window_secs))
            .max()
            .unwrap_or(Duration::from_secs(60));

        // Clean up old requests outside the longest window
        self.cleanup_expired(max_window);

        // Check each tier - if any is exceeded, rate limit the request
        for tier in &limit.tiers {
            let window = Duration::from_secs(tier.window_secs);
            let cutoff = now - window;

            // Count requests in this tier's window
            let requests_in_window = self.requests.iter().filter(|&&t| t > cutoff).count();

            if requests_in_window >= tier.max_requests as usize {
                // This tier is exceeded - apply exponential backoff
                self.violations += 1;

                // Use the tier's window as base penalty
                let base_penalty = Duration::from_secs(tier.window_secs);
                let penalty_multiplier = backoff_multiplier.powi(self.violations as i32 - 1);
                let penalty = base_penalty.mul_f64(penalty_multiplier);

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

        // All tiers passed - record this request
        self.requests.push(now);
        trace!(
            total_requests = self.requests.len(),
            "Request recorded within all rate limit tiers"
        );

        None
    }
}

/// In-memory rate limiting state tracker.
///
/// Maintains per-IP request history and violation counts. Uses DashMap
/// for efficient concurrent access across multiple request handlers.
#[derive(Clone, Debug)]
pub struct RateLimitState {
    config: Arc<RateLimitConfig>,
    clients: Arc<DashMap<IpAddr, ClientState>>,
}

impl RateLimitState {
    /// Create a new rate limit state with the given configuration.
    ///
    /// Initializes the in-memory tracking structures for monitoring
    /// client requests and enforcing rate limits.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config: Arc::new(config),
            clients: Arc::new(DashMap::new()),
        }
    }

    /// Check if a request from the given IP should be allowed.
    ///
    /// Returns None if the request is allowed, or Some(Duration) with
    /// the retry-after duration if the rate limit is exceeded.
    pub fn check_rate_limit(&self, ip: IpAddr, action: &RateLimitAction) -> Result<(), Duration> {
        if !self.config.enabled {
            return Ok(());
        }

        let limit = self.config.get_limit(action);

        let mut entry = self.clients.entry(ip).or_insert_with(ClientState::new);
        let client = entry.value_mut();

        // Check if currently blocked
        if let Some(remaining) = client.is_blocked() {
            debug!(
                ip = %ip,
                action = action.as_str(),
                remaining_secs = remaining.as_secs(),
                "Request blocked due to previous violations"
            );
            return Err(remaining);
        }

        // Record request and check limit
        if let Some(penalty) = client.record_request(&limit, self.config.backoff_multiplier) {
            debug!(
                ip = %ip,
                action = action.as_str(),
                penalty_secs = penalty.as_secs(),
                "Rate limit exceeded"
            );
            return Err(penalty);
        }

        Ok(())
    }

    /// Periodically clean up expired entries to prevent unbounded memory growth.
    ///
    /// Should be called periodically (e.g., every few minutes) to remove
    /// entries for IPs that haven't made requests recently.
    pub fn cleanup_expired_entries(&self) {
        let cutoff = Instant::now() - Duration::from_secs(3600); // 1 hour

        self.clients.retain(|_ip, client| {
            // Keep entries that have recent requests or are still blocked
            if let Some(blocked_until) = client.blocked_until {
                if Instant::now() < blocked_until {
                    return true;
                }
            }

            !client.requests.is_empty() && client.requests.last().map_or(false, |&t| t > cutoff)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_tier_allows_requests_under_limit() {
        let mut actions = HashMap::new();
        actions.insert(
            "test".to_string(),
            ActionRateLimit {
                tiers: vec![RateLimitTier {
                    window_secs: 60,
                    max_requests: 5,
                }],
            },
        );

        let config = RateLimitConfig {
            enabled: true,
            default_window_secs: 60,
            default_max_requests: 5,
            backoff_multiplier: 2.0,
            actions,
        };

        let state = RateLimitState::new(config);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");

        // Should allow first 5 requests
        for _ in 0..5 {
            assert!(state.check_rate_limit(ip, &action).is_ok());
        }
    }

    #[test]
    fn test_single_tier_blocks_excess_requests() {
        let mut actions = HashMap::new();
        actions.insert(
            "test".to_string(),
            ActionRateLimit {
                tiers: vec![RateLimitTier {
                    window_secs: 60,
                    max_requests: 3,
                }],
            },
        );

        let config = RateLimitConfig {
            enabled: true,
            default_window_secs: 60,
            default_max_requests: 10,
            backoff_multiplier: 2.0,
            actions,
        };

        let state = RateLimitState::new(config);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");

        // First 3 requests should succeed
        for _ in 0..3 {
            assert!(state.check_rate_limit(ip, &action).is_ok());
        }

        // 4th request should be blocked
        assert!(state.check_rate_limit(ip, &action).is_err());
    }

    #[test]
    fn test_multi_tier_catches_fast_burst() {
        let mut actions = HashMap::new();
        actions.insert(
            "test".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier {
                        window_secs: 5,
                        max_requests: 2,
                    }, // Fast tier
                    RateLimitTier {
                        window_secs: 60,
                        max_requests: 100,
                    }, // Slow tier
                ],
            },
        );

        let config = RateLimitConfig {
            enabled: true,
            default_window_secs: 60,
            default_max_requests: 100,
            backoff_multiplier: 2.0,
            actions,
        };

        let state = RateLimitState::new(config);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");

        // First 2 requests in 5s window should succeed
        assert!(state.check_rate_limit(ip, &action).is_ok());
        assert!(state.check_rate_limit(ip, &action).is_ok());

        // 3rd request should be blocked by fast tier
        assert!(state.check_rate_limit(ip, &action).is_err());
    }

    #[test]
    fn test_multi_tier_allows_normal_rate() {
        let mut actions = HashMap::new();
        actions.insert(
            "test".to_string(),
            ActionRateLimit {
                tiers: vec![
                    RateLimitTier {
                        window_secs: 5,
                        max_requests: 100,
                    },
                    RateLimitTier {
                        window_secs: 60,
                        max_requests: 200,
                    },
                ],
            },
        );

        let config = RateLimitConfig {
            enabled: true,
            default_window_secs: 60,
            default_max_requests: 100,
            backoff_multiplier: 2.0,
            actions,
        };

        let state = RateLimitState::new(config);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");

        // Should be able to make many requests without hitting the limits
        for _ in 0..50 {
            assert!(
                state.check_rate_limit(ip, &action).is_ok(),
                "Request should succeed with permissive rate limits"
            );
        }
    }

    #[test]
    fn test_disabled_rate_limiting() {
        let config = RateLimitConfig {
            enabled: false,
            default_window_secs: 60,
            default_max_requests: 1,
            backoff_multiplier: 2.0,
            actions: HashMap::new(),
        };

        let state = RateLimitState::new(config);
        let ip = "127.0.0.1".parse().unwrap();
        let action = RateLimitAction::new("test");

        // Should allow unlimited requests when disabled
        for _ in 0..100 {
            assert!(state.check_rate_limit(ip, &action).is_ok());
        }
    }

    #[test]
    fn test_action_specific_limits() {
        let mut actions = HashMap::new();
        actions.insert(
            "strict".to_string(),
            ActionRateLimit {
                tiers: vec![RateLimitTier {
                    window_secs: 60,
                    max_requests: 2,
                }],
            },
        );

        let config = RateLimitConfig {
            enabled: true,
            default_window_secs: 60,
            default_max_requests: 100, // Increased to allow 10 requests
            backoff_multiplier: 2.0,
            actions,
        };

        let state = RateLimitState::new(config);
        let ip = "127.0.0.1".parse().unwrap();
        let strict_action = RateLimitAction::new("strict");
        let normal_action = RateLimitAction::new("normal");

        // Strict action should allow only 2 requests
        assert!(state.check_rate_limit(ip, &strict_action).is_ok());
        assert!(state.check_rate_limit(ip, &strict_action).is_ok());
        assert!(state.check_rate_limit(ip, &strict_action).is_err());

        // Normal action should allow more (different IP to avoid interference)
        let ip2 = "127.0.0.2".parse().unwrap();
        for _ in 0..10 {
            assert!(state.check_rate_limit(ip2, &normal_action).is_ok());
        }
        // 11th request should be blocked (exceeds 60s limit of 10 derived from default_max_requests)
        assert!(state.check_rate_limit(ip2, &normal_action).is_err());
    }
}
