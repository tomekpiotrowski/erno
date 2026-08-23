//! The public snapshot.
//!
//! Docs: docs/src/content/docs/monitoring/status-page.md
//!
//! This is what the status page actually reads. It is deliberately a plain
//! JSON document rather than an API call, because the page must keep working
//! when the collector does not — and a status page that is down during an
//! outage is worse than having none, since people conclude nothing is wrong.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Overall banner state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicState {
    /// Everything is fine.
    Operational,
    /// Working, but not properly.
    Degraded,
    /// Some of it is not working.
    PartialOutage,
    /// None of it is working.
    MajorOutage,
    /// Planned work.
    Maintenance,
}

impl PublicState {
    /// Stable representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Operational => "operational",
            Self::Degraded => "degraded",
            Self::PartialOutage => "partial_outage",
            Self::MajorOutage => "major_outage",
            Self::Maintenance => "maintenance",
        }
    }

    /// Parse from the stored representation.
    #[must_use]
    pub fn from_str_or_operational(value: &str) -> Self {
        match value {
            "degraded" => Self::Degraded,
            "partial_outage" => Self::PartialOutage,
            "major_outage" => Self::MajorOutage,
            "maintenance" => Self::Maintenance,
            _ => Self::Operational,
        }
    }
}

impl std::fmt::Display for PublicState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One component as the public sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicComponent {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub state: PublicState,
    /// Successful probes over total across the history window, when the
    /// component follows a check.
    pub uptime_ratio: Option<f64>,
    /// Newest last, one entry per day.
    pub history: Vec<DayUptime>,
}

/// One day of a component's record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayUptime {
    /// `YYYY-MM-DD`.
    pub day: String,
    /// Successful probes over total that day, or `None` if it was not measured.
    pub ratio: Option<f64>,
}

/// An incident as the public sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicIncident {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub impact: String,
    pub started_at: NaiveDateTime,
    pub resolved_at: Option<NaiveDateTime>,
    pub updates: Vec<PublicIncidentUpdate>,
}

/// One timeline entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicIncidentUpdate {
    pub status: String,
    pub body: String,
    pub created_at: NaiveDateTime,
}

/// The whole document the status page fetches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    /// Product or organisation name shown in the header.
    pub name: String,
    /// Banner state, the worst across components.
    pub state: PublicState,
    /// When this document was built. The page uses it to decide whether what
    /// it is showing can still be trusted.
    pub generated_at: NaiveDateTime,
    /// How often a fresh document is expected, so the page can judge staleness
    /// without hard-coding the publisher's schedule.
    pub refresh_seconds: u64,
    pub components: Vec<PublicComponent>,
    /// Unresolved, newest first.
    pub active_incidents: Vec<PublicIncident>,
    /// Recently resolved, newest first.
    pub recent_incidents: Vec<PublicIncident>,
}

/// Roll component states up into one banner state.
///
/// Deliberately conservative: if every component that has an opinion is out,
/// the banner says major outage; if only some are, partial. Anything else that
/// is not fully operational reads as degraded.
#[must_use]
pub fn overall_state(components: &[PublicComponent]) -> PublicState {
    if components.is_empty() {
        return PublicState::Operational;
    }

    let outages = components
        .iter()
        .filter(|c| c.state == PublicState::MajorOutage || c.state == PublicState::PartialOutage)
        .count();

    if outages == components.len() {
        return PublicState::MajorOutage;
    }
    if outages > 0 {
        return PublicState::PartialOutage;
    }
    if components
        .iter()
        .any(|c| c.state == PublicState::Maintenance)
    {
        return PublicState::Maintenance;
    }
    if components.iter().any(|c| c.state == PublicState::Degraded) {
        return PublicState::Degraded;
    }
    PublicState::Operational
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(state: PublicState) -> PublicComponent {
        PublicComponent {
            id: Uuid::nil(),
            name: "API".to_string(),
            description: None,
            state,
            uptime_ratio: None,
            history: Vec::new(),
        }
    }

    #[test]
    fn nothing_configured_reads_as_operational() {
        assert_eq!(overall_state(&[]), PublicState::Operational);
    }

    #[test]
    fn all_operational_is_operational() {
        let components = vec![
            component(PublicState::Operational),
            component(PublicState::Operational),
        ];
        assert_eq!(overall_state(&components), PublicState::Operational);
    }

    #[test]
    fn one_component_out_is_a_partial_outage() {
        let components = vec![
            component(PublicState::MajorOutage),
            component(PublicState::Operational),
        ];
        assert_eq!(overall_state(&components), PublicState::PartialOutage);
    }

    #[test]
    fn everything_out_is_a_major_outage() {
        let components = vec![
            component(PublicState::MajorOutage),
            component(PublicState::PartialOutage),
        ];
        assert_eq!(overall_state(&components), PublicState::MajorOutage);
    }

    #[test]
    fn degraded_only_reads_as_degraded() {
        let components = vec![
            component(PublicState::Degraded),
            component(PublicState::Operational),
        ];
        assert_eq!(overall_state(&components), PublicState::Degraded);
    }

    #[test]
    fn maintenance_outranks_degraded_but_not_an_outage() {
        let components = vec![
            component(PublicState::Maintenance),
            component(PublicState::Degraded),
        ];
        assert_eq!(overall_state(&components), PublicState::Maintenance);

        let components = vec![
            component(PublicState::Maintenance),
            component(PublicState::MajorOutage),
        ];
        assert_eq!(overall_state(&components), PublicState::PartialOutage);
    }

    #[test]
    fn states_round_trip_through_their_stored_form() {
        for state in [
            PublicState::Operational,
            PublicState::Degraded,
            PublicState::PartialOutage,
            PublicState::MajorOutage,
            PublicState::Maintenance,
        ] {
            assert_eq!(PublicState::from_str_or_operational(state.as_str()), state);
        }
        // Anything unrecognised must not read as an outage.
        assert_eq!(
            PublicState::from_str_or_operational("nonsense"),
            PublicState::Operational
        );
    }
}
