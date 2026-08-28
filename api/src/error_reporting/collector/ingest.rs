//! Batching, grouping, and persistence.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Two statements per flush, regardless of how many reports arrived:
//!
//! 1. A multi-row upsert into `error_issue`, one row per *distinct fingerprint*.
//! 2. One multi-row insert into `error_event`.
//!
//! Aggregating by fingerprint first is not merely an optimisation. Postgres
//! refuses an `ON CONFLICT DO UPDATE` whose `VALUES` list contains the same
//! conflict key twice ("cannot affect row a second time"), so a batch that
//! contained two occurrences of one error would fail outright without it.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::NaiveDateTime;
use sea_orm::{
    ActiveValue::Set, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    Statement, TransactionTrait, Value,
};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::models::{error_event, error_issue};
use crate::error_reporting::{
    config::CollectorConfig,
    fingerprint::{self, FingerprintInput},
    CapturedError, Level, Source,
};

/// Longest stored issue title.
const MAX_TITLE_LEN: usize = 500;

/// One row of the issue upsert: every report in the batch that shared a
/// fingerprint, collapsed into a single statement row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueAggregate {
    /// Owning project.
    pub project_id: Uuid,
    /// Grouping key.
    pub fingerprint: String,
    /// Reporting component.
    pub source: Source,
    /// Highest severity seen for this fingerprint in this batch.
    pub level: Level,
    /// Exception type of the most recent occurrence.
    pub error_type: String,
    /// Human label, from the most recent occurrence's message.
    pub title: String,
    /// Top in-app frame, when there was one.
    pub culprit: Option<String>,
    /// How many occurrences this batch contributes — including ones whose
    /// event rows were shed by the burst cap.
    pub times_seen: i64,
    /// Earliest occurrence in this batch.
    pub first_seen: NaiveDateTime,
    /// Latest occurrence in this batch.
    pub last_seen: NaiveDateTime,
    /// Release of the earliest occurrence.
    pub first_release: Option<String>,
    /// Release of the latest occurrence.
    pub last_release: Option<String>,
    /// Environment of the latest occurrence.
    pub environment: Option<String>,
}

/// A report that survived the burst cap and will become a row.
#[derive(Debug, Clone)]
pub struct PreparedEvent {
    /// Owning project.
    pub project_id: Uuid,
    /// Which issue it belongs to.
    pub fingerprint: String,
    /// The report itself.
    pub report: CapturedError,
}

/// The outcome of grouping one batch.
#[derive(Debug, Clone, Default)]
pub struct PreparedBatch {
    /// One entry per distinct fingerprint, ordered deterministically.
    pub issues: Vec<IssueAggregate>,
    /// Event rows to write.
    pub events: Vec<PreparedEvent>,
    /// Occurrences counted but not stored, because of the burst cap.
    pub dropped_burst: usize,
}

/// An issue that this flush created for the first time — the alert signal.
#[derive(Debug, Clone)]
pub struct NewIssue {
    /// Row id.
    pub id: Uuid,
    /// Grouping key.
    pub fingerprint: String,
    /// Severity, for `min_level` filtering.
    pub level: Level,
    /// Reporting component.
    pub source: Source,
    /// Exception type.
    pub error_type: String,
    /// Human label.
    pub title: String,
    /// Top in-app frame.
    pub culprit: Option<String>,
    /// Release it was born in.
    pub release: Option<String>,
    /// Environment it was born in.
    pub environment: Option<String>,
}

/// A captured report tagged with the project it authenticates as.
#[derive(Debug, Clone)]
pub struct QueuedReport {
    /// Owning project.
    pub project_id: Uuid,
    /// Project slug, for metrics.
    pub project_slug: String,
    /// The report itself.
    pub report: CapturedError,
}

/// Group a batch by (project, fingerprint) and apply the burst cap.
///
/// Pure: no database, no clock, no configuration beyond the cap itself.
#[must_use]
pub fn prepare_batch(reports: Vec<QueuedReport>, config: &CollectorConfig) -> PreparedBatch {
    let mut aggregates: HashMap<(Uuid, String), IssueAggregate> = HashMap::new();
    // Preserves arrival order of first sighting, so output is deterministic.
    let mut order: Vec<(Uuid, String)> = Vec::new();
    let mut stored_per_fingerprint: HashMap<(Uuid, String), usize> = HashMap::new();
    let mut events: Vec<PreparedEvent> = Vec::new();
    let mut dropped_burst = 0usize;

    for queued in reports {
        let project_id = queued.project_id;
        let report = queued.report;
        let key = fingerprint::fingerprint(&FingerprintInput {
            project_id,
            source: report.source,
            error_type: &report.error_type,
            message: &report.message,
            frames: &report.frames,
            client_fingerprint: report.client_fingerprint.as_deref(),
            call_site: report.call_site(),
        });
        let map_key = (project_id, key.clone());

        match aggregates.get_mut(&map_key) {
            Some(existing) => existing.absorb(&report),
            None => {
                order.push(map_key.clone());
                aggregates.insert(
                    map_key.clone(),
                    IssueAggregate::from_report(project_id, &key, &report),
                );
            }
        }

        // The burst cap bounds stored rows while counts still accumulate in
        // full, so a render loop costs a fixed number of writes per flush.
        let stored = stored_per_fingerprint.entry(map_key).or_insert(0);
        if *stored < config.max_events_per_flush_per_issue {
            *stored += 1;
            events.push(PreparedEvent {
                project_id,
                fingerprint: key,
                report,
            });
        } else {
            dropped_burst += 1;
        }
    }

    let issues = order
        .into_iter()
        .filter_map(|key| aggregates.remove(&key))
        .collect();

    PreparedBatch {
        issues,
        events,
        dropped_burst,
    }
}

impl IssueAggregate {
    fn from_report(project_id: Uuid, key: &str, report: &CapturedError) -> Self {
        Self {
            project_id,
            fingerprint: key.to_string(),
            source: report.source,
            level: report.level,
            error_type: report.error_type.clone(),
            title: title_from(&report.message),
            culprit: fingerprint::culprit(&report.frames),
            times_seen: 1,
            first_seen: report.timestamp,
            last_seen: report.timestamp,
            first_release: report.release.clone(),
            last_release: report.release.clone(),
            environment: report.environment.clone(),
        }
    }

    fn absorb(&mut self, report: &CapturedError) {
        self.times_seen += 1;

        // Severity only ever climbs: an issue that has been fatal stays fatal.
        if report.level.rank() > self.level.rank() {
            self.level = report.level;
        }

        if report.timestamp < self.first_seen {
            self.first_seen = report.timestamp;
            self.first_release.clone_from(&report.release);
        }
        // `>=` so the newest report in a same-millisecond batch wins the
        // "latest" fields, which matches what an operator expects to see.
        if report.timestamp >= self.last_seen {
            self.last_seen = report.timestamp;
            self.error_type.clone_from(&report.error_type);
            self.title = title_from(&report.message);
            if report.release.is_some() {
                self.last_release.clone_from(&report.release);
            }
            if report.environment.is_some() {
                self.environment.clone_from(&report.environment);
            }
            if let Some(culprit) = fingerprint::culprit(&report.frames) {
                self.culprit = Some(culprit);
            }
        }
    }
}

fn title_from(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.chars().count() <= MAX_TITLE_LEN {
        return trimmed.to_string();
    }
    trimmed.chars().take(MAX_TITLE_LEN).collect()
}

/// Number of columns in the issue upsert's `VALUES` rows.
const ISSUE_COLUMNS: usize = 13;

/// Write one prepared batch: the issue upsert, then the event insert. Returns
/// the issues that were created for the first time.
///
/// Deliberately does **not** open a transaction of its own — it runs on
/// whatever connection or transaction the caller hands it. Opening one here
/// would commit any enclosing transaction along with it, which silently breaks
/// callers that batch work (the test harness wraps every case in a transaction
/// it expects to roll back). Use [`write_batch_transactional`] for the
/// standalone case.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] if either statement fails. Callers must not
/// report that failure through the error reporter.
pub async fn write_batch<C: ConnectionTrait>(
    db: &C,
    config: &CollectorConfig,
    batch: PreparedBatch,
) -> Result<Vec<NewIssue>, DbErr> {
    if batch.issues.is_empty() {
        return Ok(Vec::new());
    }

    let upserted = upsert_issues(db, &batch.issues).await?;
    insert_events(db, config, &batch, &upserted).await?;

    let new_issues = batch
        .issues
        .iter()
        .filter_map(|aggregate| {
            let outcome = upserted.get(&aggregate.fingerprint)?;
            // Only alert on a genuinely new fingerprint that has never alerted.
            (outcome.inserted && outcome.alert_sent_at.is_none()).then(|| NewIssue {
                id: outcome.id,
                fingerprint: aggregate.fingerprint.clone(),
                level: aggregate.level,
                source: aggregate.source,
                error_type: aggregate.error_type.clone(),
                title: aggregate.title.clone(),
                culprit: aggregate.culprit.clone(),
                release: aggregate.first_release.clone(),
                environment: aggregate.environment.clone(),
            })
        })
        .collect();

    metrics::counter!("erno_error_reports_written_total").increment(batch.events.len() as u64);
    Ok(new_issues)
}

/// Write a batch inside its own transaction, so the issue upsert and the event
/// insert land together or not at all.
///
/// Used by the background writer, which owns its connection. The inline path
/// deliberately does not use this — see [`write_batch`].
///
/// # Errors
///
/// Returns the underlying [`DbErr`] if either statement or the transaction fails.
pub async fn write_batch_transactional(
    db: &DatabaseConnection,
    config: &CollectorConfig,
    batch: PreparedBatch,
) -> Result<Vec<NewIssue>, DbErr> {
    let txn = db.begin().await?;
    let new_issues = write_batch(&txn, config, batch).await?;
    txn.commit().await?;
    Ok(new_issues)
}

/// What the upsert reported back about one fingerprint.
#[derive(Debug, Clone)]
struct UpsertOutcome {
    id: Uuid,
    inserted: bool,
    alert_sent_at: Option<NaiveDateTime>,
}

async fn upsert_issues<C: ConnectionTrait>(
    db: &C,
    issues: &[IssueAggregate],
) -> Result<HashMap<String, UpsertOutcome>, DbErr> {
    // Sorted by the conflict target so every writer takes the unique index's
    // row locks in the same order. Mixing fingerprint-only sort with the
    // composite conflict target deadlocks under multi-replica ingest.
    let mut ordered: Vec<&IssueAggregate> = issues.iter().collect();
    ordered.sort_unstable_by(|a, b| {
        a.project_id
            .cmp(&b.project_id)
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
    });

    let mut placeholders = Vec::with_capacity(ordered.len());
    let mut values: Vec<Value> = Vec::with_capacity(ordered.len() * ISSUE_COLUMNS);

    for (row, issue) in ordered.iter().enumerate() {
        let base = row * ISSUE_COLUMNS;
        let row_placeholders: Vec<String> = (1..=ISSUE_COLUMNS)
            .map(|c| format!("${}", base + c))
            .collect();
        placeholders.push(format!("({})", row_placeholders.join(", ")));

        values.push(issue.project_id.into());
        values.push(issue.fingerprint.clone().into());
        values.push(issue.source.as_str().into());
        values.push(issue.error_type.clone().into());
        values.push(issue.title.clone().into());
        values.push(issue.culprit.clone().into());
        values.push(issue.level.as_str().into());
        values.push(issue.times_seen.into());
        values.push(issue.first_seen.into());
        values.push(issue.last_seen.into());
        values.push(issue.first_release.clone().into());
        values.push(issue.last_release.clone().into());
        values.push(issue.environment.clone().into());
    }

    // `xmax = 0` is the standard way to tell an insert from an update inside an
    // upsert, and it is what makes new-issue alerting free of a second query.
    //
    // `first_release` is deliberately absent from the DO UPDATE set: it must
    // stay the release the issue was born in.
    let sql = format!(
        "INSERT INTO error_issue \
         (project_id, fingerprint, source, error_type, title, culprit, level, times_seen, \
          first_seen, last_seen, first_release, last_release, environment) \
         VALUES {} \
         ON CONFLICT (project_id, fingerprint) DO UPDATE SET \
           times_seen   = error_issue.times_seen + EXCLUDED.times_seen, \
           last_seen    = GREATEST(error_issue.last_seen, EXCLUDED.last_seen), \
           first_seen   = LEAST(error_issue.first_seen, EXCLUDED.first_seen), \
           error_type   = EXCLUDED.error_type, \
           title        = EXCLUDED.title, \
           culprit      = COALESCE(EXCLUDED.culprit, error_issue.culprit), \
           last_release = COALESCE(EXCLUDED.last_release, error_issue.last_release), \
           environment  = COALESCE(EXCLUDED.environment, error_issue.environment), \
           level        = CASE \
                            WHEN array_position(ARRAY['warning','error','fatal'], EXCLUDED.level) \
                               > array_position(ARRAY['warning','error','fatal'], error_issue.level) \
                            THEN EXCLUDED.level ELSE error_issue.level END, \
           status       = CASE \
                            WHEN error_issue.status = 'resolved' \
                             AND EXCLUDED.last_seen > COALESCE(error_issue.resolved_at, error_issue.first_seen) \
                            THEN 'unresolved' ELSE error_issue.status END, \
           resolved_at  = CASE \
                            WHEN error_issue.status = 'resolved' \
                             AND EXCLUDED.last_seen > COALESCE(error_issue.resolved_at, error_issue.first_seen) \
                            THEN NULL ELSE error_issue.resolved_at END \
         RETURNING id, fingerprint, times_seen, (xmax = 0) AS inserted, alert_sent_at",
        placeholders.join(", ")
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            values,
        ))
        .await?;

    let mut outcomes = HashMap::with_capacity(rows.len());
    let mut created = 0u64;
    for row in rows {
        let fingerprint: String = row.try_get("", "fingerprint")?;
        let outcome = UpsertOutcome {
            id: row.try_get("", "id")?,
            inserted: row.try_get("", "inserted")?,
            alert_sent_at: row.try_get("", "alert_sent_at")?,
        };
        if outcome.inserted {
            created += 1;
        }
        outcomes.insert(fingerprint, outcome);
    }

    if created > 0 {
        metrics::counter!("erno_error_issues_created_total").increment(created);
    }
    Ok(outcomes)
}

async fn insert_events<C: ConnectionTrait>(
    db: &C,
    config: &CollectorConfig,
    batch: &PreparedBatch,
    upserted: &HashMap<String, UpsertOutcome>,
) -> Result<(), DbErr> {
    let models: Vec<error_event::ActiveModel> = batch
        .events
        .iter()
        .filter_map(|event| {
            let issue = upserted.get(&event.fingerprint)?;
            let report = &event.report;
            Some(error_event::ActiveModel {
                id: Set(Uuid::new_v4()),
                project_id: Set(event.project_id),
                issue_id: Set(issue.id),
                source: Set(report.source.as_str().to_string()),
                level: Set(report.level.as_str().to_string()),
                error_type: Set(report.error_type.clone()),
                message: Set(report.message.clone()),
                stack: Set(report.stack.clone()),
                frames: Set(if report.frames.is_empty() {
                    None
                } else {
                    serde_json::to_value(&report.frames).ok()
                }),
                context: Set(report.context.clone()),
                release: Set(report.release.clone()),
                environment: Set(report.environment.clone()),
                user_id: Set(report.user_id),
                user_email: Set(report.user_email.clone()),
                client_ip: Set(if config.store_client_ip {
                    report.client_ip.clone()
                } else {
                    None
                }),
                created_at: Set(report.timestamp),
            })
        })
        .collect();

    if models.is_empty() {
        return Ok(());
    }

    error_event::Entity::insert_many(models).exec(db).await?;
    Ok(())
}

/// Mark issues as alerted, so a restart or a parallel writer cannot alert twice.
///
/// # Errors
///
/// Returns the underlying [`DbErr`] when the update fails.
pub async fn mark_alerted(db: &DatabaseConnection, ids: &[Uuid]) -> Result<(), DbErr> {
    use sea_orm::{ColumnTrait, QueryFilter};

    if ids.is_empty() {
        return Ok(());
    }
    error_issue::Entity::update_many()
        .col_expr(
            error_issue::Column::AlertSentAt,
            sea_orm::sea_query::Expr::value(chrono::Utc::now().naive_utc()),
        )
        .filter(error_issue::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

/// Where accepted reports go.
///
/// [`CollectorSink::Sync`] writes on the caller's own connection. That is what
/// tests use: each case runs inside a single connection's transaction that is
/// rolled back afterwards, so a background writer on a second connection would
/// both deadlock the pool and never see the test's data.
#[derive(Clone)]
pub enum CollectorSink {
    /// Write inline, on the request's connection.
    Sync,
    /// Hand off to the background writer.
    Channel(mpsc::Sender<QueuedReport>),
}

impl std::fmt::Debug for CollectorSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sync => f.write_str("CollectorSink::Sync"),
            Self::Channel(_) => f.write_str("CollectorSink::Channel"),
        }
    }
}

impl CollectorSink {
    /// Build the sink a configuration calls for, spawning the writer — and,
    /// when alerting is configured, a separate task to mail about new issues.
    ///
    /// Alerting is a separate task on purpose: SMTP is slow, and the write loop
    /// must never wait on it.
    #[must_use]
    pub fn start(
        db: DatabaseConnection,
        config: Arc<CollectorConfig>,
        alerts: Option<(crate::mailer::Mailer, super::alerts::AlertContext)>,
    ) -> Self {
        let alert_tx = alerts.map(|(mailer, context)| {
            let (tx, rx) = mpsc::channel::<Vec<NewIssue>>(64);
            tokio::spawn(super::alerts::alert_loop(db.clone(), mailer, context, rx));
            tx
        });

        if config.sync_writes {
            return Self::Sync;
        }
        let (tx, rx) = mpsc::channel(config.queue_capacity.max(1));
        tokio::spawn(writer_loop(db, config, rx, alert_tx));
        Self::Channel(tx)
    }

    /// Accept reports, returning `(accepted, dropped)`.
    ///
    /// Never blocks and never fails: a full queue sheds the newest reports and
    /// counts them, because under a runaway loop the queue is already saturated
    /// with the same fingerprint and the newest report carries nothing new.
    pub async fn accept(
        &self,
        db: &DatabaseConnection,
        config: &CollectorConfig,
        project_id: Uuid,
        project_slug: &str,
        reports: Vec<CapturedError>,
    ) -> (usize, usize) {
        let queued: Vec<QueuedReport> = reports
            .into_iter()
            .map(|report| QueuedReport {
                project_id,
                project_slug: project_slug.to_string(),
                report,
            })
            .collect();
        match self {
            Self::Sync => {
                let count = queued.len();
                let batch = prepare_batch(queued, config);
                match write_batch(db, config, batch).await {
                    Ok(_) => (count, 0),
                    Err(e) => {
                        // Never `tracing::error!` here: the capture layer would
                        // turn a collector outage into a self-feeding loop.
                        eprintln!("error_reporting: inline write failed: {e}");
                        metrics::counter!("erno_error_report_write_failures_total").increment(1);
                        (0, count)
                    }
                }
            }
            Self::Channel(tx) => {
                let mut accepted = 0;
                let mut dropped = 0;
                for report in queued {
                    match tx.try_send(report) {
                        Ok(()) => accepted += 1,
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            dropped += 1;
                            metrics::counter!(
                                "erno_error_reports_dropped_total",
                                "reason" => "queue_full"
                            )
                            .increment(1);
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            dropped += 1;
                            metrics::counter!(
                                "erno_error_reports_dropped_total",
                                "reason" => "closed"
                            )
                            .increment(1);
                        }
                    }
                }
                (accepted, dropped)
            }
        }
    }
}

/// Background writer: parks on `recv` when idle, then accumulates until the
/// batch is full or the flush interval elapses.
async fn writer_loop(
    db: DatabaseConnection,
    config: Arc<CollectorConfig>,
    mut rx: mpsc::Receiver<QueuedReport>,
    alert_tx: Option<mpsc::Sender<Vec<NewIssue>>>,
) {
    let flush_interval = std::time::Duration::from_millis(config.flush_interval_ms.max(1));

    loop {
        // An idle collector does no work at all — no polling, no timer.
        let Some(first) = rx.recv().await else {
            break;
        };

        let mut reports = vec![first];
        let deadline = tokio::time::sleep(flush_interval);
        tokio::pin!(deadline);
        let mut closed = false;

        loop {
            tokio::select! {
                () = &mut deadline => break,
                message = rx.recv() => match message {
                    Some(report) => {
                        reports.push(report);
                        if reports.len() >= config.batch_size {
                            break;
                        }
                    }
                    None => {
                        closed = true;
                        break;
                    }
                },
            }
        }

        let batch = prepare_batch(reports, &config);
        if batch.dropped_burst > 0 {
            metrics::counter!("erno_error_reports_dropped_total", "reason" => "burst_cap")
                .increment(batch.dropped_burst as u64);
        }

        match write_batch_transactional(&db, &config, batch).await {
            Ok(new_issues) => {
                if let (Some(tx), false) = (alert_tx.as_ref(), new_issues.is_empty()) {
                    // Hand off without waiting; a full alert queue means the
                    // mailer is struggling, and reports matter more than mail.
                    if tx.try_send(new_issues).is_err() {
                        metrics::counter!(
                            "erno_error_alert_emails_total",
                            "result" => "dropped"
                        )
                        .increment(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("error_reporting: batch write failed: {e}");
                metrics::counter!("erno_error_report_write_failures_total").increment(1);
            }
        }

        if closed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn config(burst_cap: usize) -> CollectorConfig {
        CollectorConfig {
            max_events_per_flush_per_issue: burst_cap,
            ..CollectorConfig::default()
        }
    }

    fn report(message: &str, at: NaiveDateTime) -> QueuedReport {
        let mut captured = CapturedError::new(
            Source::App,
            Level::Error,
            "TypeError".to_string(),
            message.to_string(),
        );
        captured.timestamp = at;
        QueuedReport {
            project_id: Uuid::from_u128(1),
            project_slug: "test".to_string(),
            report: captured,
        }
    }

    #[test]
    fn identical_reports_collapse_to_one_issue_with_a_full_count() {
        let now = Utc::now().naive_utc();
        let reports: Vec<_> = (0..500).map(|_| report("boom", now)).collect();
        let batch = prepare_batch(reports, &config(10));

        assert_eq!(batch.issues.len(), 1, "one fingerprint, one upsert row");
        assert_eq!(batch.issues[0].times_seen, 500, "every occurrence counted");
        assert_eq!(batch.events.len(), 10, "burst cap bounds stored rows");
        assert_eq!(batch.dropped_burst, 490);
    }

    #[test]
    fn distinct_errors_stay_separate() {
        let now = Utc::now().naive_utc();
        let batch = prepare_batch(
            vec![report("boom", now), report("totally different", now)],
            &config(10),
        );
        assert_eq!(batch.issues.len(), 2);
        assert_eq!(batch.events.len(), 2);
        assert_eq!(batch.dropped_burst, 0);
    }

    #[test]
    fn the_burst_cap_is_per_fingerprint_not_per_batch() {
        let now = Utc::now().naive_utc();
        let mut reports: Vec<_> = (0..30).map(|_| report("a", now)).collect();
        reports.extend((0..30).map(|_| report("b", now)));
        let batch = prepare_batch(reports, &config(10));

        assert_eq!(batch.issues.len(), 2);
        assert_eq!(batch.events.len(), 20, "10 stored for each fingerprint");
        assert_eq!(batch.dropped_burst, 40);
    }

    #[test]
    fn first_and_last_seen_span_the_batch_regardless_of_arrival_order() {
        let now = Utc::now().naive_utc();
        let early = now - Duration::minutes(10);
        let late = now + Duration::minutes(10);
        // Deliberately out of order.
        let batch = prepare_batch(
            vec![
                report("boom", now),
                report("boom", late),
                report("boom", early),
            ],
            &config(10),
        );
        assert_eq!(batch.issues[0].first_seen, early);
        assert_eq!(batch.issues[0].last_seen, late);
    }

    #[test]
    fn first_release_tracks_the_earliest_occurrence_and_last_the_newest() {
        let now = Utc::now().naive_utc();
        let mut old = report("boom", now - Duration::minutes(5));
        old.report.release = Some("1.0.0".to_string());
        let mut new = report("boom", now);
        new.report.release = Some("2.0.0".to_string());

        let batch = prepare_batch(vec![new, old], &config(10));
        assert_eq!(batch.issues[0].first_release.as_deref(), Some("1.0.0"));
        assert_eq!(batch.issues[0].last_release.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn severity_only_climbs() {
        let now = Utc::now().naive_utc();
        let mut warning = report("boom", now);
        warning.report.level = Level::Warning;
        let mut fatal = report("boom", now + Duration::seconds(1));
        fatal.report.level = Level::Fatal;
        let mut later_warning = report("boom", now + Duration::seconds(2));
        later_warning.report.level = Level::Warning;

        let batch = prepare_batch(vec![warning, fatal, later_warning], &config(10));
        assert_eq!(
            batch.issues[0].level,
            Level::Fatal,
            "an issue that has been fatal stays fatal"
        );
    }

    #[test]
    fn the_title_comes_from_the_most_recent_occurrence() {
        let now = Utc::now().naive_utc();
        // Same fingerprint (numbers are normalised away), different text.
        let batch = prepare_batch(
            vec![
                report("failed for user 1", now),
                report("failed for user 2", now + Duration::seconds(1)),
            ],
            &config(10),
        );
        assert_eq!(batch.issues.len(), 1);
        assert_eq!(batch.issues[0].title, "failed for user 2");
    }

    #[test]
    fn a_long_message_is_truncated_into_the_title() {
        let now = Utc::now().naive_utc();
        let long = "word ".repeat(1000);
        let batch = prepare_batch(vec![report(&long, now)], &config(10));
        assert_eq!(batch.issues[0].title.chars().count(), MAX_TITLE_LEN);
    }

    #[test]
    fn issue_order_is_deterministic_by_first_sighting() {
        let now = Utc::now().naive_utc();
        let batch = prepare_batch(
            vec![
                report("zeta", now),
                report("alpha", now),
                report("zeta", now),
            ],
            &config(10),
        );
        assert_eq!(batch.issues.len(), 2);
        assert_eq!(batch.issues[0].title, "zeta");
        assert_eq!(batch.issues[1].title, "alpha");
    }

    #[test]
    fn an_empty_batch_produces_nothing() {
        let batch = prepare_batch(vec![], &config(10));
        assert!(batch.issues.is_empty());
        assert!(batch.events.is_empty());
    }

    #[test]
    fn different_sources_never_merge() {
        let now = Utc::now().naive_utc();
        let mut api = report("boom", now);
        api.report.source = Source::Api;
        let app = report("boom", now);
        let batch = prepare_batch(vec![api, app], &config(10));
        assert_eq!(batch.issues.len(), 2);
    }
}
