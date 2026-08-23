//! Timing helper for framework operations.
//!
//! Docs: docs/src/content/docs/monitoring/metrics.md
//!
//! Exists so every timed operation produces the same metric shape — a duration
//! histogram plus a counter, both labelled by *what* was done and whether it
//! worked. Without a shared helper these drift: one seam labels its outcome
//! `status`, another `result`, a third forgets to count failures at all, and
//! the dashboards end up unable to compare them.

use std::time::Instant;

/// Times one operation and records it on drop-free, explicit completion.
///
/// Deliberately not `Drop`-based: an operation that was cancelled mid-flight is
/// not the same as one that succeeded or failed, and silently recording it as
/// either would be worse than not recording it.
#[must_use = "a timer that is never finished records nothing"]
pub struct OperationTimer {
    duration_metric: &'static str,
    count_metric: &'static str,
    kind_label: &'static str,
    kind: String,
    start: Instant,
}

impl OperationTimer {
    /// Begin timing.
    ///
    /// `kind` is the low-cardinality dimension — a job type, a storage backend,
    /// an entity name. It must never be something unbounded like a user id or a
    /// file name, or the metric store will choke on the label cardinality.
    pub fn start(
        duration_metric: &'static str,
        count_metric: &'static str,
        kind_label: &'static str,
        kind: impl Into<String>,
    ) -> Self {
        Self {
            duration_metric,
            count_metric,
            kind_label,
            kind: kind.into(),
            start: Instant::now(),
        }
    }

    /// Record the operation as finished, taking the outcome from a `Result`.
    pub fn finish<T, E>(self, result: &Result<T, E>) {
        self.finish_with(if result.is_ok() { "ok" } else { "error" });
    }

    /// Record the operation as finished with an explicit outcome label.
    pub fn finish_with(self, outcome: &'static str) {
        let elapsed = self.start.elapsed().as_secs_f64();

        metrics::histogram!(
            self.duration_metric,
            self.kind_label => self.kind.clone(),
            "outcome" => outcome,
        )
        .record(elapsed);

        metrics::counter!(
            self.count_metric,
            self.kind_label => self.kind,
            "outcome" => outcome,
        )
        .increment(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timer_records_success_and_failure_without_panicking() {
        // No recorder is installed in unit tests; the macros must still be safe
        // to call, so this asserts the helper does not blow up on its own.
        let ok: Result<(), ()> = Ok(());
        OperationTimer::start("d", "c", "kind", "thing").finish(&ok);

        let err: Result<(), ()> = Err(());
        OperationTimer::start("d", "c", "kind", "thing").finish(&err);

        OperationTimer::start("d", "c", "kind", "thing").finish_with("timeout");
    }

    #[test]
    fn the_kind_label_accepts_owned_and_borrowed_values() {
        OperationTimer::start("d", "c", "kind", "borrowed").finish_with("ok");
        OperationTimer::start("d", "c", "kind", String::from("owned")).finish_with("ok");
    }
}
