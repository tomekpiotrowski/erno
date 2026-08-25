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
///
/// The tracing span is created here so every timed operation is a child of the
/// current request (or job). It is *not* entered: entering would make
/// `OperationTimer` `!Send` across `.await`. sqlx events therefore stay on the
/// HTTP/job parent; the child still records the operation's duration on drop.
#[must_use = "a timer that is never finished records nothing"]
pub struct OperationTimer {
    duration_metric: &'static str,
    count_metric: &'static str,
    kind_label: &'static str,
    kind: String,
    start: Instant,
    span: tracing::Span,
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
        let kind = kind.into();
        let span = tracing::info_span!(
            "operation",
            otel.name = duration_metric,
            kind = kind.as_str(),
            otel.status_code = tracing::field::Empty,
            outcome = tracing::field::Empty,
        );
        Self {
            duration_metric,
            count_metric,
            kind_label,
            kind,
            start: Instant::now(),
            span,
        }
    }

    /// Record the operation as finished, taking the outcome from a `Result`.
    pub fn finish<T, E>(self, result: &Result<T, E>) {
        self.finish_with(if result.is_ok() { "ok" } else { "error" });
    }

    /// Record the operation as finished with an explicit outcome label.
    pub fn finish_with(self, outcome: &'static str) {
        let elapsed = self.start.elapsed().as_secs_f64();
        self.span.record("outcome", outcome);
        self.span.record(
            "otel.status_code",
            if outcome == "ok" { "OK" } else { "ERROR" },
        );

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
    fn finish_sets_span_status_and_drop_still_exports() {
        use opentelemetry::trace::TracerProvider as _;
        use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
        use tracing_subscriber::layer::SubscriberExt;

        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = provider.tracer("test");
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
        let _guard = tracing::subscriber::set_default(subscriber);

        OperationTimer::start("sync", "c", "kind", "deck").finish_with("ok");
        drop(OperationTimer::start("sync", "c", "kind", "abandoned"));

        provider.force_flush().unwrap();
        let spans = exporter.get_finished_spans().unwrap();
        use opentelemetry::trace::Status;
        assert!(
            spans
                .iter()
                .any(|s| s.name.as_ref() == "sync" && s.status == Status::Ok),
            "finished timer should be Ok, got {spans:?}"
        );
        assert!(
            spans
                .iter()
                .any(|s| s.name.as_ref() == "sync" && s.status == Status::Unset),
            "dropped timer should still export, got {spans:?}"
        );
    }

    #[test]
    fn the_kind_label_accepts_owned_and_borrowed_values() {
        OperationTimer::start("d", "c", "kind", "borrowed").finish_with("ok");
        OperationTimer::start("d", "c", "kind", String::from("owned")).finish_with("ok");
    }
}
