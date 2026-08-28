//! Probe outcomes and the flap-damping state machine.
//!
//! Docs: docs/src/content/docs/monitoring/uptime.md
//!
//! Pure: no HTTP, no database, no clock. Flap damping is the part that decides
//! whether a monitoring platform is trusted or muted, so it is worth being able
//! to test exhaustively.

use serde::{Deserialize, Serialize};

/// Whether a check is currently passing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    /// Never probed yet.
    Unknown,
    /// Passing.
    Up,
    /// Failing, past the threshold.
    Down,
}

impl CheckState {
    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Up => "up",
            Self::Down => "down",
        }
    }

    /// Parse from the database representation.
    #[must_use]
    pub fn from_str_or_unknown(value: &str) -> Self {
        match value {
            "up" => Self::Up,
            "down" => Self::Down,
            _ => Self::Unknown,
        }
    }
}

impl std::fmt::Display for CheckState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one probe attempt observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    /// Whether every assertion held.
    pub ok: bool,
    /// HTTP status, when a response arrived at all.
    pub status_code: Option<u16>,
    /// How long it took.
    pub duration_ms: u32,
    /// Why it failed, in a sentence.
    pub error: Option<String>,
}

impl ProbeOutcome {
    /// A successful probe.
    #[must_use]
    pub const fn success(status_code: u16, duration_ms: u32) -> Self {
        Self {
            ok: true,
            status_code: Some(status_code),
            duration_ms,
            error: None,
        }
    }

    /// A failed probe.
    #[must_use]
    pub fn failure(status_code: Option<u16>, duration_ms: u32, error: impl Into<String>) -> Self {
        Self {
            ok: false,
            status_code,
            duration_ms,
            error: Some(error.into()),
        }
    }
}

/// How a check's stored state should change after a probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateTransition {
    /// The state to store.
    pub state: CheckState,
    /// The failure run to store.
    pub consecutive_failures: i32,
    /// Whether the state actually changed, which is what an incident hangs on.
    pub changed: bool,
}

/// Apply a probe result to a check's current state.
///
/// Asymmetric on purpose: going **down** needs `failure_threshold` consecutive
/// failures, because one dropped packet must not page anyone at 3am. Coming
/// **up** needs a single success, because a service that is answering is
/// answering and there is no value in making people wait to hear it.
#[must_use]
pub fn apply_probe(
    current: CheckState,
    consecutive_failures: i32,
    failure_threshold: i32,
    ok: bool,
) -> StateTransition {
    // A threshold below 1 would make a check flap on every blip.
    let threshold = failure_threshold.max(1);

    if ok {
        return StateTransition {
            state: CheckState::Up,
            consecutive_failures: 0,
            changed: current != CheckState::Up,
        };
    }

    let failures = consecutive_failures.saturating_add(1);
    if failures >= threshold {
        StateTransition {
            state: CheckState::Down,
            consecutive_failures: failures,
            changed: current != CheckState::Down,
        }
    } else {
        // Not yet confirmed: stay where we are, but remember the run. A check
        // that has never passed stays `unknown` rather than claiming to be up.
        StateTransition {
            state: current,
            consecutive_failures: failures,
            changed: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_failure_does_not_take_a_check_down() {
        // The whole reason flap damping exists.
        let t = apply_probe(CheckState::Up, 0, 2, false);
        assert_eq!(t.state, CheckState::Up);
        assert_eq!(t.consecutive_failures, 1);
        assert!(!t.changed);
    }

    #[test]
    fn the_threshold_failure_takes_it_down() {
        let t = apply_probe(CheckState::Up, 1, 2, false);
        assert_eq!(t.state, CheckState::Down);
        assert_eq!(t.consecutive_failures, 2);
        assert!(t.changed, "this is the transition an incident hangs on");
    }

    #[test]
    fn a_single_success_brings_it_back() {
        // Asymmetric on purpose: recovery should be believed immediately.
        let t = apply_probe(CheckState::Down, 7, 2, true);
        assert_eq!(t.state, CheckState::Up);
        assert_eq!(t.consecutive_failures, 0);
        assert!(t.changed);
    }

    #[test]
    fn staying_down_is_not_a_change() {
        let t = apply_probe(CheckState::Down, 5, 2, false);
        assert_eq!(t.state, CheckState::Down);
        assert!(!t.changed, "an incident must not reopen on every probe");
    }

    #[test]
    fn staying_up_is_not_a_change() {
        let t = apply_probe(CheckState::Up, 0, 2, true);
        assert!(!t.changed);
    }

    #[test]
    fn a_never_probed_check_does_not_claim_to_be_up() {
        let t = apply_probe(CheckState::Unknown, 0, 3, false);
        assert_eq!(t.state, CheckState::Unknown);
        assert_eq!(t.consecutive_failures, 1);
    }

    #[test]
    fn a_threshold_of_one_goes_down_immediately() {
        let t = apply_probe(CheckState::Up, 0, 1, false);
        assert_eq!(t.state, CheckState::Down);
        assert!(t.changed);
    }

    #[test]
    fn a_nonsense_threshold_is_clamped_rather_than_flapping() {
        for threshold in [0, -5] {
            let t = apply_probe(CheckState::Up, 0, threshold, false);
            assert_eq!(t.state, CheckState::Down, "threshold {threshold}");
        }
    }

    #[test]
    fn a_flapping_service_settles_correctly() {
        // up -> fail -> fail (down) -> ok (up) -> fail (still up)
        let mut state = CheckState::Up;
        let mut failures = 0;

        for (ok, expected) in [
            (false, CheckState::Up),
            (false, CheckState::Down),
            (true, CheckState::Up),
            (false, CheckState::Up),
        ] {
            let t = apply_probe(state, failures, 2, ok);
            state = t.state;
            failures = t.consecutive_failures;
            assert_eq!(state, expected);
        }
    }

    #[test]
    fn the_failure_counter_saturates() {
        let t = apply_probe(CheckState::Down, i32::MAX, 2, false);
        assert_eq!(t.consecutive_failures, i32::MAX);
    }
}
