//! Alert rule evaluation: the state machine.
//!
//! Docs: docs/src/content/docs/monitoring/alerts.md
//!
//! Pure — no database, no clock beyond what is passed in, no notifications.
//! Alert fatigue is how monitoring platforms actually die, and every mechanism
//! that prevents it lives here, so all of it can be tested exhaustively.

use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// Where a rule reads its number from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSource {
    /// The error store: new issues, or event volume.
    Errors,
    /// Uptime checks: how many are down.
    Uptime,
    /// Application health readings: how many instances are unhealthy.
    Subsystem,
}

impl RuleSource {
    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Errors => "errors",
            Self::Uptime => "uptime",
            Self::Subsystem => "subsystem",
        }
    }

    /// Parse from the database representation.
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "errors" => Some(Self::Errors),
            "uptime" => Some(Self::Uptime),
            "subsystem" => Some(Self::Subsystem),
            _ => None,
        }
    }
}

/// How the observed value is compared with the threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Comparator {
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
}

impl Comparator {
    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gt => "gt",
            Self::Gte => "gte",
            Self::Lt => "lt",
            Self::Lte => "lte",
        }
    }

    /// Parse, defaulting to `gt` — the overwhelmingly common case.
    #[must_use]
    pub fn from_str_or_gt(value: &str) -> Self {
        match value {
            "gte" => Self::Gte,
            "lt" => Self::Lt,
            "lte" => Self::Lte,
            _ => Self::Gt,
        }
    }

    /// Whether the observation breaches the threshold.
    #[must_use]
    pub fn breaches(self, value: f64, threshold: f64) -> bool {
        match self {
            Self::Gt => value > threshold,
            Self::Gte => value >= threshold,
            Self::Lt => value < threshold,
            Self::Lte => value <= threshold,
        }
    }
}

/// Where a rule is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleState {
    /// Not breaching.
    Ok,
    /// Breaching, but not yet for long enough to be believed.
    Pending,
    /// Breaching and confirmed.
    Firing,
}

impl RuleState {
    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Pending => "pending",
            Self::Firing => "firing",
        }
    }

    /// Parse from the database representation.
    #[must_use]
    pub fn from_str_or_ok(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "firing" => Self::Firing,
            _ => Self::Ok,
        }
    }
}

/// What, if anything, to tell someone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notify {
    /// Say nothing.
    Nothing,
    /// It started.
    Firing,
    /// It stopped. Sent so nobody chases a problem that has already gone.
    Resolved,
}

/// The outcome of evaluating one rule once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    /// The state to store.
    pub state: RuleState,
    /// When the rule entered that state.
    pub state_since: Option<NaiveDateTime>,
    /// What to send.
    pub notify: Notify,
}

/// Everything the state machine needs to know about a rule's current position.
#[derive(Debug, Clone, Copy)]
pub struct RuleStatus {
    /// Stored state.
    pub state: RuleState,
    /// When it entered that state.
    pub state_since: Option<NaiveDateTime>,
    /// When someone was last told about it.
    pub last_notified_at: Option<NaiveDateTime>,
    /// Suppressed until this time, if an operator silenced it.
    pub silence_until: Option<NaiveDateTime>,
}

/// Timing knobs.
#[derive(Debug, Clone, Copy)]
pub struct RuleTiming {
    /// How long a breach must persist before it is believed.
    pub for_seconds: i64,
    /// How often to repeat a notification while a rule stays firing.
    pub repeat_seconds: i64,
}

/// Advance one rule.
///
/// The three things that keep this from becoming noise:
///
/// * `for_seconds` — a single bad scrape does not page anyone.
/// * `repeat_seconds` — a rule that stays firing re-notifies on a schedule
///   rather than on every evaluation.
/// * `silence_until` — an operator who already knows is not told again.
#[must_use]
pub fn advance(
    status: RuleStatus,
    timing: RuleTiming,
    breaching: bool,
    now: NaiveDateTime,
) -> Transition {
    let silenced = status.silence_until.is_some_and(|until| now < until);

    if !breaching {
        // Only announce recovery for something that was actually announced.
        let notify = if status.state == RuleState::Firing && !silenced {
            Notify::Resolved
        } else {
            Notify::Nothing
        };
        return Transition {
            state: RuleState::Ok,
            state_since: if status.state == RuleState::Ok {
                status.state_since
            } else {
                Some(now)
            },
            notify,
        };
    }

    match status.state {
        RuleState::Ok => {
            // A breach that must persist starts as pending; one that need not
            // is confirmed immediately.
            if timing.for_seconds <= 0 {
                Transition {
                    state: RuleState::Firing,
                    state_since: Some(now),
                    notify: if silenced {
                        Notify::Nothing
                    } else {
                        Notify::Firing
                    },
                }
            } else {
                Transition {
                    state: RuleState::Pending,
                    state_since: Some(now),
                    notify: Notify::Nothing,
                }
            }
        }
        RuleState::Pending => {
            let held_long_enough = status.state_since.is_none_or(|since| {
                now.signed_duration_since(since) >= Duration::seconds(timing.for_seconds)
            });
            if held_long_enough {
                Transition {
                    state: RuleState::Firing,
                    state_since: Some(now),
                    notify: if silenced {
                        Notify::Nothing
                    } else {
                        Notify::Firing
                    },
                }
            } else {
                Transition {
                    state: RuleState::Pending,
                    state_since: status.state_since,
                    notify: Notify::Nothing,
                }
            }
        }
        RuleState::Firing => {
            let due_to_repeat = timing.repeat_seconds > 0
                && status.last_notified_at.is_none_or(|last| {
                    now.signed_duration_since(last) >= Duration::seconds(timing.repeat_seconds)
                });
            Transition {
                state: RuleState::Firing,
                state_since: status.state_since,
                notify: if due_to_repeat && !silenced {
                    Notify::Firing
                } else {
                    Notify::Nothing
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(seconds: i64) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 8, 23)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            + Duration::seconds(seconds)
    }

    fn status(state: RuleState, since: Option<i64>, notified: Option<i64>) -> RuleStatus {
        RuleStatus {
            state,
            state_since: since.map(at),
            last_notified_at: notified.map(at),
            silence_until: None,
        }
    }

    const TIMING: RuleTiming = RuleTiming {
        for_seconds: 120,
        repeat_seconds: 3_600,
    };

    #[test]
    fn a_single_bad_reading_does_not_notify_anyone() {
        // The reason `for_seconds` exists.
        let t = advance(status(RuleState::Ok, None, None), TIMING, true, at(0));
        assert_eq!(t.state, RuleState::Pending);
        assert_eq!(t.notify, Notify::Nothing);
    }

    #[test]
    fn a_breach_that_persists_starts_firing() {
        let t = advance(
            status(RuleState::Pending, Some(0), None),
            TIMING,
            true,
            at(120),
        );
        assert_eq!(t.state, RuleState::Firing);
        assert_eq!(t.notify, Notify::Firing);
    }

    #[test]
    fn a_breach_that_clears_before_the_hold_never_notifies() {
        let t = advance(
            status(RuleState::Pending, Some(0), None),
            TIMING,
            false,
            at(60),
        );
        assert_eq!(t.state, RuleState::Ok);
        assert_eq!(
            t.notify,
            Notify::Nothing,
            "nobody was told it started, so nobody is told it stopped"
        );
    }

    #[test]
    fn recovery_is_announced_for_something_that_was_announced() {
        let t = advance(
            status(RuleState::Firing, Some(0), Some(0)),
            TIMING,
            false,
            at(500),
        );
        assert_eq!(t.state, RuleState::Ok);
        assert_eq!(t.notify, Notify::Resolved);
    }

    #[test]
    fn a_firing_rule_does_not_notify_on_every_evaluation() {
        // Without this a rule that stays firing mails every 30 seconds.
        let t = advance(
            status(RuleState::Firing, Some(0), Some(0)),
            TIMING,
            true,
            at(60),
        );
        assert_eq!(t.state, RuleState::Firing);
        assert_eq!(t.notify, Notify::Nothing);
    }

    #[test]
    fn a_firing_rule_repeats_on_schedule() {
        let t = advance(
            status(RuleState::Firing, Some(0), Some(0)),
            TIMING,
            true,
            at(3_600),
        );
        assert_eq!(t.notify, Notify::Firing);
    }

    #[test]
    fn a_zero_repeat_interval_means_notify_once_only() {
        let timing = RuleTiming {
            for_seconds: 0,
            repeat_seconds: 0,
        };
        let t = advance(
            status(RuleState::Firing, Some(0), Some(0)),
            timing,
            true,
            at(100_000),
        );
        assert_eq!(t.notify, Notify::Nothing);
    }

    #[test]
    fn a_zero_hold_fires_immediately() {
        let timing = RuleTiming {
            for_seconds: 0,
            repeat_seconds: 3_600,
        };
        let t = advance(status(RuleState::Ok, None, None), timing, true, at(0));
        assert_eq!(t.state, RuleState::Firing);
        assert_eq!(t.notify, Notify::Firing);
    }

    #[test]
    fn a_silence_suppresses_notifications_but_not_state() {
        let mut s = status(RuleState::Pending, Some(0), None);
        s.silence_until = Some(at(10_000));

        let t = advance(s, TIMING, true, at(200));
        assert_eq!(t.state, RuleState::Firing, "state still tracks reality");
        assert_eq!(t.notify, Notify::Nothing, "but nobody is told");
    }

    #[test]
    fn a_silence_that_has_expired_no_longer_suppresses() {
        let mut s = status(RuleState::Pending, Some(0), None);
        s.silence_until = Some(at(100));

        let t = advance(s, TIMING, true, at(200));
        assert_eq!(t.notify, Notify::Firing);
    }

    #[test]
    fn a_silenced_recovery_is_not_announced_either() {
        // Otherwise silencing a rule still produces a "resolved" message.
        let mut s = status(RuleState::Firing, Some(0), Some(0));
        s.silence_until = Some(at(10_000));

        let t = advance(s, TIMING, false, at(500));
        assert_eq!(t.state, RuleState::Ok);
        assert_eq!(t.notify, Notify::Nothing);
    }

    #[test]
    fn a_flapping_signal_only_notifies_once_per_confirmed_episode() {
        let timing = RuleTiming {
            for_seconds: 60,
            repeat_seconds: 3_600,
        };
        let mut current = status(RuleState::Ok, None, None);
        let mut notifications = 0;

        // Breaching for five minutes with one brief blip of recovery.
        for (second, breaching) in [
            (0, true),
            (30, true),
            (60, true), // confirmed -> notify
            (90, true),
            (120, true),
            (150, false), // recovered -> notify
            (180, true),
            (240, true), // confirmed again -> notify
        ] {
            let t = advance(current, timing, breaching, at(second));
            if t.notify != Notify::Nothing {
                notifications += 1;
                current.last_notified_at = Some(at(second));
            }
            current.state = t.state;
            current.state_since = t.state_since;
        }

        assert_eq!(
            notifications, 3,
            "fired, resolved, fired again — not one per evaluation"
        );
    }

    #[test]
    fn comparators_do_what_they_say() {
        assert!(Comparator::Gt.breaches(5.0, 4.0));
        assert!(!Comparator::Gt.breaches(4.0, 4.0));
        assert!(Comparator::Gte.breaches(4.0, 4.0));
        assert!(Comparator::Lt.breaches(3.0, 4.0));
        assert!(Comparator::Lte.breaches(4.0, 4.0));
        // Anything unrecognised must not silently invert the meaning.
        assert_eq!(Comparator::from_str_or_gt("nonsense"), Comparator::Gt);
    }
}
