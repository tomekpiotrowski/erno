//! Failure isolation for the outbound reporter.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! This exists because reporting now crosses the network. Without it, an
//! unreachable collector means every flush interval starts another request that
//! will hang until it times out — a slow leak of tasks and sockets in the
//! application, caused entirely by the thing that was supposed to be watching
//! it. The reporter becoming the outage is the failure mode that matters most
//! here, and it did not exist when reports were written locally.
//!
//! Time is passed in rather than read, so the behaviour is testable without
//! sleeping.

use std::time::{Duration, Instant};

/// Trips after a run of failures and refuses work until a cooldown elapses.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    consecutive_failures: u32,
    open_until: Option<Instant>,
    threshold: u32,
    cooldown: Duration,
}

impl CircuitBreaker {
    /// A breaker that opens after `threshold` consecutive failures and stays
    /// open for `cooldown`.
    ///
    /// A `threshold` of zero disables the breaker, which is the honest reading
    /// of "never trip" rather than "trip immediately".
    #[must_use]
    pub const fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            consecutive_failures: 0,
            open_until: None,
            threshold,
            cooldown,
        }
    }

    /// Whether a request may be attempted at `now`.
    #[must_use]
    pub fn allows(&self, now: Instant) -> bool {
        match self.open_until {
            Some(until) => now >= until,
            None => true,
        }
    }

    /// Whether the breaker is currently tripped.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open_until.is_some()
    }

    /// Record a success: the breaker closes and the failure run resets.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.open_until = None;
    }

    /// Record a failure, opening the breaker once the threshold is reached.
    pub fn record_failure(&mut self, now: Instant) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.threshold > 0 && self.consecutive_failures >= self.threshold {
            self.open_until = Some(now + self.cooldown);
        }
    }

    /// Consecutive failures since the last success.
    #[must_use]
    pub const fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaker() -> CircuitBreaker {
        CircuitBreaker::new(3, Duration::from_secs(60))
    }

    #[test]
    fn a_fresh_breaker_allows_requests() {
        let now = Instant::now();
        assert!(breaker().allows(now));
        assert!(!breaker().is_open());
    }

    #[test]
    fn it_opens_only_after_the_threshold_run() {
        let now = Instant::now();
        let mut b = breaker();

        b.record_failure(now);
        b.record_failure(now);
        assert!(b.allows(now), "two failures is below the threshold");

        b.record_failure(now);
        assert!(b.is_open());
        assert!(!b.allows(now), "further requests are refused immediately");
    }

    #[test]
    fn it_closes_again_after_the_cooldown() {
        let now = Instant::now();
        let mut b = breaker();
        for _ in 0..3 {
            b.record_failure(now);
        }

        assert!(!b.allows(now + Duration::from_secs(59)));
        assert!(b.allows(now + Duration::from_secs(60)), "cooldown elapsed");
    }

    #[test]
    fn a_success_resets_the_run_and_closes_it() {
        let now = Instant::now();
        let mut b = breaker();
        b.record_failure(now);
        b.record_failure(now);
        b.record_success();
        assert_eq!(b.consecutive_failures(), 0);

        // The run restarts from scratch, so two more failures still do not trip it.
        b.record_failure(now);
        b.record_failure(now);
        assert!(b.allows(now));
    }

    #[test]
    fn a_zero_threshold_never_trips() {
        let now = Instant::now();
        let mut b = CircuitBreaker::new(0, Duration::from_secs(60));
        for _ in 0..1_000 {
            b.record_failure(now);
        }
        assert!(b.allows(now), "zero means disabled, not trip-immediately");
    }

    #[test]
    fn the_failure_counter_saturates_instead_of_wrapping() {
        let now = Instant::now();
        let mut b = CircuitBreaker::new(u32::MAX, Duration::from_secs(1));
        b.consecutive_failures = u32::MAX - 1;
        b.record_failure(now);
        b.record_failure(now);
        assert_eq!(b.consecutive_failures(), u32::MAX);
    }
}
