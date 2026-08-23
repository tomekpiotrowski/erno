//! Retry pacing for the outbound reporter.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Nothing else in the codebase has exponential backoff — `ErnoRealtimeService`
//! uses a flat delay — so this is deliberately local to the reporter rather
//! than a shared utility invented for one caller.

use std::time::Duration;

/// First retry delay.
pub const BASE_DELAY_MS: u64 = 1_000;
/// Ceiling on the delay, however many failures have accumulated.
pub const MAX_DELAY_MS: u64 = 60_000;
/// Fraction of the delay randomised, to stop every replica retrying in lockstep.
pub const JITTER_FRACTION: f64 = 0.2;

/// Delay before retry number `attempt` (1 = the first retry).
///
/// Doubles each time up to [`MAX_DELAY_MS`], then applies ±20% jitter. A
/// deployment with many API replicas would otherwise synchronise its retries
/// into a thundering herd against a collector that is already struggling.
#[must_use]
pub fn next_delay(attempt: u32) -> Duration {
    Duration::from_millis(jittered(base_delay_ms(attempt)))
}

/// The un-jittered delay, exposed so tests can assert the schedule exactly.
#[must_use]
pub fn base_delay_ms(attempt: u32) -> u64 {
    if attempt == 0 {
        return 0;
    }
    // Saturating shift: attempt 64+ would otherwise wrap to zero.
    let shift = (attempt - 1).min(u64::BITS - 1);
    BASE_DELAY_MS
        .saturating_mul(1u64 << shift)
        .min(MAX_DELAY_MS)
}

fn jittered(delay_ms: u64) -> u64 {
    if delay_ms == 0 {
        return 0;
    }
    let spread = (delay_ms as f64 * JITTER_FRACTION) as u64;
    if spread == 0 {
        return delay_ms;
    }
    // Uniform in [delay - spread, delay + spread].
    let offset = fastrand::u64(0..=spread.saturating_mul(2));
    delay_ms.saturating_add(offset).saturating_sub(spread)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schedule_doubles_then_flattens() {
        assert_eq!(base_delay_ms(1), 1_000);
        assert_eq!(base_delay_ms(2), 2_000);
        assert_eq!(base_delay_ms(3), 4_000);
        assert_eq!(base_delay_ms(4), 8_000);
        assert_eq!(base_delay_ms(5), 16_000);
        assert_eq!(base_delay_ms(6), 32_000);
        assert_eq!(base_delay_ms(7), MAX_DELAY_MS);
        assert_eq!(base_delay_ms(8), MAX_DELAY_MS);
    }

    #[test]
    fn a_huge_attempt_count_stays_at_the_ceiling() {
        // Guards the shift: `1 << 64` is undefined and would wrap to a tiny delay.
        assert_eq!(base_delay_ms(1_000), MAX_DELAY_MS);
        assert_eq!(base_delay_ms(u32::MAX), MAX_DELAY_MS);
    }

    #[test]
    fn attempt_zero_does_not_wait() {
        assert_eq!(base_delay_ms(0), 0);
        assert_eq!(next_delay(0), Duration::ZERO);
    }

    #[test]
    fn jitter_stays_within_bounds_and_actually_varies() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            let delay = next_delay(5).as_millis() as u64;
            let base = base_delay_ms(5);
            let spread = (base as f64 * JITTER_FRACTION) as u64;
            assert!(
                delay >= base - spread && delay <= base + spread,
                "{delay} outside [{}, {}]",
                base - spread,
                base + spread
            );
            seen.insert(delay);
        }
        assert!(seen.len() > 1, "jitter must actually randomise");
    }
}
