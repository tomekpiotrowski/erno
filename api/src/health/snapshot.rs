//! What "healthy" means for an Erno application.
//!
//! Docs: docs/src/content/docs/monitoring/subsystem-health.md
//!
//! These are the signals a generic monitoring tool cannot produce, because they
//! require knowing how Erno works: that jobs move through a `pending →
//! running → completed` lifecycle, that a `running` row older than its timeout
//! means a worker died, that `sync_push_queue` backing up means clients are
//! about to see stale data.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// A point-in-time reading of one application instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSnapshot {
    /// Identifies the reporting process. Several instances report separately.
    pub instance: String,
    /// Application version.
    pub release: Option<String>,
    /// Deployment environment.
    pub environment: String,
    /// When the reading was taken, UTC.
    pub reported_at: NaiveDateTime,
    /// Background job queue.
    pub jobs: JobHealth,
    /// Offline sync fan-out.
    pub sync: SyncHealth,
    /// Connection pool.
    pub database: DatabaseHealth,
    /// Live push connections.
    pub websocket: WebSocketHealth,
}

/// Background job queue health.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobHealth {
    /// Waiting to run for the first time.
    pub pending: i64,
    /// Waiting to be retried after a transient failure.
    pub pending_retry: i64,
    /// Currently claimed by a worker.
    pub running: i64,
    /// Permanently failed in the last hour.
    pub failed_last_hour: i64,
    /// Age of the oldest waiting job.
    ///
    /// Depth alone is ambiguous — a thousand jobs enqueued a second ago is
    /// fine. Age is not ambiguous.
    pub oldest_pending_age_seconds: Option<i64>,
    /// Claimed but not updated for longer than the configured timeout.
    ///
    /// Almost always a worker that died holding the row, which nothing else
    /// will notice: the job simply never finishes.
    pub stuck_running: i64,
}

/// Sync fan-out health.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncHealth {
    /// Rows waiting to be pushed to connected clients.
    pub push_queue_depth: i64,
    /// Age of the oldest unpushed row.
    pub oldest_push_age_seconds: Option<i64>,
}

/// Connection pool health.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseHealth {
    /// Connections currently open.
    pub pool_size: u32,
    /// Connections currently idle.
    pub pool_idle: u32,
}

/// Live connection health.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebSocketHealth {
    /// Users with at least one open socket.
    pub connections: i64,
}

/// How a subsystem is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    /// Nothing to see.
    Ok,
    /// Worth a look, not worth waking anyone.
    Degraded,
    /// Something is broken.
    Down,
}

impl HealthState {
    /// Stable representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Degraded => "degraded",
            Self::Down => "down",
        }
    }

    /// The worse of two states.
    #[must_use]
    pub fn worst(self, other: Self) -> Self {
        self.max(other)
    }
}

impl std::fmt::Display for HealthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Thresholds that turn numbers into states.
///
/// Deliberately few and deliberately generous. A monitoring platform that cries
/// wolf in its first week gets filtered out permanently, so the defaults here
/// are set where a human would genuinely want to look.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthThresholds {
    /// Oldest waiting job before the queue is considered degraded.
    pub job_age_degraded_seconds: i64,
    /// Oldest waiting job before it is considered down.
    pub job_age_down_seconds: i64,
    /// Permanent failures in an hour before the queue is degraded.
    pub job_failures_degraded: i64,
    /// Sync backlog before it is degraded.
    pub sync_depth_degraded: i64,
    /// Age of the oldest unpushed sync row before it is degraded.
    pub sync_age_degraded_seconds: i64,
    /// How long a heartbeat may be missing before the instance is down.
    pub heartbeat_stale_seconds: i64,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            job_age_degraded_seconds: 120,
            job_age_down_seconds: 600,
            job_failures_degraded: 10,
            sync_depth_degraded: 10_000,
            sync_age_degraded_seconds: 300,
            heartbeat_stale_seconds: 180,
        }
    }
}

/// One subsystem's verdict, ready for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsystemStatus {
    /// Subsystem name, e.g. `jobs`.
    pub name: String,
    /// Its state.
    pub state: HealthState,
    /// Why, in a sentence an operator can act on.
    pub detail: String,
}

impl HealthSnapshot {
    /// Evaluate every subsystem against the thresholds.
    ///
    /// Pure: the whole policy is a function of a snapshot and a config, which
    /// is what makes it testable without a database.
    #[must_use]
    pub fn evaluate(&self, thresholds: &HealthThresholds) -> Vec<SubsystemStatus> {
        vec![
            self.evaluate_jobs(thresholds),
            self.evaluate_sync(thresholds),
            self.evaluate_database(),
            self.evaluate_websocket(),
        ]
    }

    /// The worst state across all subsystems.
    #[must_use]
    pub fn overall(&self, thresholds: &HealthThresholds) -> HealthState {
        self.evaluate(thresholds)
            .into_iter()
            .fold(HealthState::Ok, |worst, s| worst.worst(s.state))
    }

    fn evaluate_jobs(&self, thresholds: &HealthThresholds) -> SubsystemStatus {
        let jobs = &self.jobs;
        let waiting = jobs.pending + jobs.pending_retry;

        // A stuck row means a worker died mid-job; nothing else surfaces that.
        if jobs.stuck_running > 0 {
            return SubsystemStatus {
                name: "jobs".to_string(),
                state: HealthState::Down,
                detail: format!(
                    "{} job(s) claimed by a worker that stopped reporting",
                    jobs.stuck_running
                ),
            };
        }

        if let Some(age) = jobs.oldest_pending_age_seconds {
            if age >= thresholds.job_age_down_seconds {
                return SubsystemStatus {
                    name: "jobs".to_string(),
                    state: HealthState::Down,
                    detail: format!(
                        "oldest of {waiting} waiting job(s) has been queued for {}",
                        humanize(age)
                    ),
                };
            }
            if age >= thresholds.job_age_degraded_seconds {
                return SubsystemStatus {
                    name: "jobs".to_string(),
                    state: HealthState::Degraded,
                    detail: format!(
                        "oldest of {waiting} waiting job(s) has been queued for {}",
                        humanize(age)
                    ),
                };
            }
        }

        if jobs.failed_last_hour >= thresholds.job_failures_degraded {
            return SubsystemStatus {
                name: "jobs".to_string(),
                state: HealthState::Degraded,
                detail: format!(
                    "{} job(s) failed permanently in the last hour",
                    jobs.failed_last_hour
                ),
            };
        }

        SubsystemStatus {
            name: "jobs".to_string(),
            state: HealthState::Ok,
            detail: format!("{waiting} waiting, {} running", jobs.running),
        }
    }

    fn evaluate_sync(&self, thresholds: &HealthThresholds) -> SubsystemStatus {
        let sync = &self.sync;

        // Sync failing quietly is the worst Erno failure mode: clients keep
        // working against data that is silently out of date.
        let stale = sync
            .oldest_push_age_seconds
            .is_some_and(|age| age >= thresholds.sync_age_degraded_seconds);

        if stale || sync.push_queue_depth >= thresholds.sync_depth_degraded {
            return SubsystemStatus {
                name: "sync".to_string(),
                state: HealthState::Degraded,
                detail: match sync.oldest_push_age_seconds {
                    Some(age) => format!(
                        "{} row(s) waiting to push, oldest {}",
                        sync.push_queue_depth,
                        humanize(age)
                    ),
                    None => format!("{} row(s) waiting to push", sync.push_queue_depth),
                },
            };
        }

        SubsystemStatus {
            name: "sync".to_string(),
            state: HealthState::Ok,
            detail: format!("{} row(s) waiting to push", sync.push_queue_depth),
        }
    }

    fn evaluate_database(&self) -> SubsystemStatus {
        let db = &self.database;
        // Reported, never judged. Erno holds connections for the whole process
        // lifetime — the sync listener, the websocket listener, each job worker
        // — so zero idle is the normal steady state, not saturation. A single
        // reading cannot tell the two apart, and flagging it turns the healthy
        // case amber forever, which is how a monitoring tool teaches people to
        // ignore it.
        //
        // Sustained saturation is a question for a rule over time series, not
        // for a point-in-time verdict.
        SubsystemStatus {
            name: "database".to_string(),
            state: HealthState::Ok,
            detail: format!("{} connection(s), {} idle", db.pool_size, db.pool_idle),
        }
    }

    fn evaluate_websocket(&self) -> SubsystemStatus {
        SubsystemStatus {
            name: "websocket".to_string(),
            state: HealthState::Ok,
            detail: format!("{} connected user(s)", self.websocket.connections),
        }
    }
}

/// Render a duration the way an operator reads it.
#[must_use]
pub fn humanize(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3_600),
        s => format!("{}d", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> HealthSnapshot {
        HealthSnapshot {
            instance: "api-0".to_string(),
            release: Some("1.0.0".to_string()),
            environment: "test".to_string(),
            reported_at: chrono::Utc::now().naive_utc(),
            jobs: JobHealth::default(),
            sync: SyncHealth::default(),
            database: DatabaseHealth {
                pool_size: 5,
                pool_idle: 4,
            },
            websocket: WebSocketHealth::default(),
        }
    }

    fn state_of(statuses: &[SubsystemStatus], name: &str) -> HealthState {
        statuses
            .iter()
            .find(|s| s.name == name)
            .expect("subsystem present")
            .state
    }

    #[test]
    fn a_quiet_system_is_healthy() {
        let statuses = snapshot().evaluate(&HealthThresholds::default());
        assert!(statuses.iter().all(|s| s.state == HealthState::Ok));
        assert_eq!(
            snapshot().overall(&HealthThresholds::default()),
            HealthState::Ok
        );
    }

    #[test]
    fn a_deep_but_fresh_queue_is_not_an_alert() {
        // Depth alone is ambiguous: a burst of just-enqueued work is normal.
        let mut s = snapshot();
        s.jobs.pending = 50_000;
        s.jobs.oldest_pending_age_seconds = Some(2);
        assert_eq!(
            state_of(&s.evaluate(&HealthThresholds::default()), "jobs"),
            HealthState::Ok
        );
    }

    #[test]
    fn an_old_queue_escalates_with_age() {
        let mut s = snapshot();
        s.jobs.pending = 3;

        s.jobs.oldest_pending_age_seconds = Some(150);
        assert_eq!(
            state_of(&s.evaluate(&HealthThresholds::default()), "jobs"),
            HealthState::Degraded
        );

        s.jobs.oldest_pending_age_seconds = Some(1_200);
        assert_eq!(
            state_of(&s.evaluate(&HealthThresholds::default()), "jobs"),
            HealthState::Down
        );
    }

    #[test]
    fn a_stuck_job_is_down_regardless_of_everything_else() {
        // A worker died holding the row; the job will never finish on its own.
        let mut s = snapshot();
        s.jobs.stuck_running = 1;
        let statuses = s.evaluate(&HealthThresholds::default());
        assert_eq!(state_of(&statuses, "jobs"), HealthState::Down);
        assert!(statuses
            .iter()
            .any(|st| st.detail.contains("stopped reporting")));
    }

    #[test]
    fn a_run_of_permanent_failures_is_degraded() {
        let mut s = snapshot();
        s.jobs.failed_last_hour = 25;
        assert_eq!(
            state_of(&s.evaluate(&HealthThresholds::default()), "jobs"),
            HealthState::Degraded
        );
    }

    #[test]
    fn sync_degrades_on_depth_or_on_age() {
        let mut deep = snapshot();
        deep.sync.push_queue_depth = 50_000;
        assert_eq!(
            state_of(&deep.evaluate(&HealthThresholds::default()), "sync"),
            HealthState::Degraded
        );

        // A small but stale backlog is just as bad: clients are working against
        // data that is quietly out of date.
        let mut stale = snapshot();
        stale.sync.push_queue_depth = 3;
        stale.sync.oldest_push_age_seconds = Some(900);
        assert_eq!(
            state_of(&stale.evaluate(&HealthThresholds::default()), "sync"),
            HealthState::Degraded
        );
    }

    #[test]
    fn a_busy_pool_is_reported_but_not_flagged() {
        // Erno holds connections for the process lifetime, so zero idle is the
        // normal steady state. Flagging it would make the healthy case amber
        // forever.
        let mut s = snapshot();
        s.database.pool_idle = 0;
        let statuses = s.evaluate(&HealthThresholds::default());
        assert_eq!(state_of(&statuses, "database"), HealthState::Ok);
        assert!(statuses
            .iter()
            .any(|st| st.name == "database" && st.detail.contains("0 idle")));
    }

    #[test]
    fn overall_is_the_worst_subsystem() {
        let mut s = snapshot();
        s.jobs.stuck_running = 1;
        assert_eq!(s.overall(&HealthThresholds::default()), HealthState::Down);

        let mut s = snapshot();
        s.sync.push_queue_depth = 50_000;
        assert_eq!(
            s.overall(&HealthThresholds::default()),
            HealthState::Degraded
        );
    }

    #[test]
    fn durations_read_the_way_an_operator_expects() {
        assert_eq!(humanize(45), "45s");
        assert_eq!(humanize(150), "2m");
        assert_eq!(humanize(7_200), "2h");
        assert_eq!(humanize(200_000), "2d");
    }
}
