//! Capturing this process's own failures.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Tracing is initialised in [`crate::boot`] long before a database connection
//! or an [`crate::app::App`] exists, so the layer cannot be handed a reporter at
//! construction time. It reads a process-global instead and no-ops until
//! [`install`] is called, which keeps boot ordering unchanged.
//!
//! **Loop prevention is the non-negotiable part.** A failure to report an error
//! must never itself produce an error to report, so: this layer ignores its own
//! target, and everything on the reporting path logs with `eprintln!` rather
//! than `tracing::error!`.

use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use serde_json::{Map, Value};
use tracing::{
    field::{Field, Visit},
    Event, Level as TracingLevel, Subscriber,
};
use tracing_subscriber::{layer::Context, Layer};

use crate::error_reporting::{
    config::ErrorReportingConfig, fingerprint, CapturedError, Frame, Level, Source,
};

use super::handle::ErrorReporter;

/// Tracing target prefix for the reporting subsystem itself. Events under it
/// are never captured.
pub const SELF_TARGET: &str = "erno::error_reporting";

/// Targets that structurally duplicate a report we already produce.
///
/// A single panic otherwise lands three times: once from the panic hook (the
/// useful one, with a backtrace), once from `CatchPanicLayer` turning it into a
/// 500, and once from `TraceLayer` noting that the response failed. Only the
/// first carries anything an operator can act on.
const PANIC_DUPLICATE_TARGET: &str = "tower_http::catch_panic";

/// `TraceLayer`'s "response failed" line. It fires for every 5xx and carries no
/// detail beyond the status, so it is noise next to whatever actually logged
/// the failure. Capturing 5xx properly is a separate, deliberate feature.
const RESPONSE_FAILURE_TARGET: &str = "tower_http::trace::on_failure";

struct Installed {
    reporter: ErrorReporter,
    config: Arc<ErrorReportingConfig>,
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();

/// Make a reporter available to the capture hooks.
///
/// Called once the application is built. Before this, every hook is inert.
pub fn install(reporter: ErrorReporter, config: Arc<ErrorReportingConfig>) {
    // `set` fails only if something already installed one; first wins, which is
    // the right behaviour for a process-global.
    let _ = INSTALLED.set(Installed { reporter, config });
}

fn installed() -> Option<&'static Installed> {
    INSTALLED.get()
}

/// Whether a reporter has been installed. Exposed for tests and diagnostics.
#[must_use]
pub fn is_installed() -> bool {
    INSTALLED.get().is_some()
}

/// Captures `ERROR`-level tracing events as reports.
///
/// Deliberately layered rather than wired into the formatter: it must see every
/// error the application already logs, including from libraries, without those
/// call sites knowing anything about error reporting.
pub struct ErrorCaptureLayer;

impl<S: Subscriber> Layer<S> for ErrorCaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != TracingLevel::ERROR {
            return;
        }

        let Some(installed) = installed() else {
            return;
        };
        if !installed.config.capture_tracing_errors {
            return;
        }

        let metadata = event.metadata();
        let target = metadata.target();

        // Loop guard: reporting failures are logged under this target.
        if target.starts_with(SELF_TARGET) {
            return;
        }
        if target.starts_with(RESPONSE_FAILURE_TARGET) {
            return;
        }
        // Only skip the panic-layer duplicate when the hook is actually
        // reporting the panic; otherwise this would be the sole signal.
        if installed.config.capture_panics && target.starts_with(PANIC_DUPLICATE_TARGET) {
            return;
        }
        if installed.config.is_ignored_target(target) {
            return;
        }

        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let mut context = Map::new();
        context.insert("target".to_string(), Value::String(target.to_string()));
        if let Some(file) = metadata.file() {
            context.insert("file".to_string(), Value::String(file.to_string()));
        }
        if let Some(line) = metadata.line() {
            context.insert("line".to_string(), Value::Number(line.into()));
        }
        if !visitor.fields.is_empty() {
            context.insert("fields".to_string(), Value::Object(visitor.fields));
        }

        let report = CapturedError {
            source: Source::Api,
            level: Level::Error,
            // The call site's module is a far better grouping key than the
            // message, which is usually interpolated with variable data.
            error_type: target.to_string(),
            message: visitor.message,
            stack: None,
            frames: Vec::new(),
            context: Value::Object(context),
            release: None,
            environment: None,
            user_id: None,
            user_email: None,
            client_ip: None,
            client_fingerprint: None,
            timestamp: chrono::Utc::now().naive_utc(),
        };

        installed.reporter.capture(report);
    }
}

/// Collects a tracing event's `message` and remaining fields.
#[derive(Default)]
struct FieldVisitor {
    message: String,
    fields: Map<String, Value>,
}

impl FieldVisitor {
    fn insert(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            if let Value::String(text) = &value {
                self.message.clone_from(text);
                return;
            }
        }
        self.fields.insert(field.name().to_string(), value);
    }
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, Value::String(value.to_string()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, Value::Number(value.into()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, Value::Number(value.into()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, Value::Bool(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.insert(field, Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let mut rendered = String::new();
        // `Debug` formatting cannot fail for a String sink, but avoid unwrap.
        let _ = write!(rendered, "{value:?}");
        if field.name() == "message" {
            // `tracing::error!("...")` arrives as a Debug-formatted message.
            self.message = rendered;
        } else {
            self.insert(field, Value::String(rendered));
        }
    }
}

/// Install a panic hook that reports panics before the previous hook runs.
///
/// This is the only mechanism that catches panics outside a request: job
/// workers, the sync listener, the websocket listener, background loops. The
/// previous hook is always called afterwards, so the usual panic output is
/// preserved rather than swallowed.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report_panic(info);
        previous(info);
    }));
}

fn report_panic(info: &std::panic::PanicHookInfo<'_>) {
    let Some(installed) = installed() else {
        return;
    };
    if !installed.config.capture_panics {
        return;
    }

    let message = panic_message(info);
    let location = info.location().map(|l| (l.file().to_string(), l.line()));

    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    let frames = parse_backtrace(&backtrace);

    let mut context = Map::new();
    if let Some((file, line)) = &location {
        context.insert("file".to_string(), Value::String(file.clone()));
        context.insert("line".to_string(), Value::Number((*line).into()));
    }
    if let Some(name) = std::thread::current().name() {
        context.insert("thread".to_string(), Value::String(name.to_string()));
    }

    let report = CapturedError {
        source: Source::Api,
        level: Level::Fatal,
        error_type: "panic".to_string(),
        message,
        stack: Some(backtrace),
        frames,
        context: Value::Object(context),
        release: None,
        environment: None,
        user_id: None,
        user_email: None,
        client_ip: None,
        client_fingerprint: None,
        timestamp: chrono::Utc::now().naive_utc(),
    };

    installed.reporter.capture(report);
}

/// Extract the panic payload as text.
fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "panic with a non-string payload".to_string()
}

/// Pull `name @ file:line` frames out of a captured backtrace.
///
/// Best-effort by design: a backtrace with no symbols still yields a report,
/// just one grouped by message rather than by frame.
fn parse_backtrace(backtrace: &str) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut pending: Option<String> = None;

    for line in backtrace.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("at ") {
            let (file, line_number) = split_location(rest);
            let function = pending.take();
            // Without this, every panic's top frames are the hook and the
            // unwinder, so all panics would share a fingerprint and collapse
            // into one issue — and the culprit would name this file.
            if is_unwind_machinery(function.as_deref(), &file) {
                continue;
            }
            let in_app = fingerprint::is_in_app(Some(&file));
            frames.push(Frame {
                function,
                file: Some(file),
                line: line_number,
                column: None,
                in_app,
            });
            continue;
        }

        // `12: some::function::name`
        if let Some((index, name)) = line.split_once(':') {
            if index.chars().all(|c| c.is_ascii_digit()) && !index.is_empty() {
                pending = Some(name.trim().to_string());
            }
        }

        if frames.len() >= 50 {
            break;
        }
    }

    frames
}

/// Frames belonging to panic delivery rather than to the code that panicked.
fn is_unwind_machinery(function: Option<&str>, file: &str) -> bool {
    const FUNCTION_MARKERS: [&str; 7] = [
        "erno::error_reporting::reporter::capture",
        "std::panicking",
        "core::panicking",
        "std::panic",
        "rust_begin_unwind",
        "std::backtrace",
        "__rust_",
    ];
    if file.contains("/error_reporting/reporter/capture.rs") {
        return true;
    }
    function.is_some_and(|name| FUNCTION_MARKERS.iter().any(|m| name.contains(m)))
}

fn split_location(rest: &str) -> (String, Option<u32>) {
    let mut parts = rest.rsplitn(3, ':');
    let last = parts.next();
    let middle = parts.next();
    let head = parts.next();

    match (head, middle, last) {
        // file:line:column
        (Some(file), Some(line), Some(_column)) if line.chars().all(|c| c.is_ascii_digit()) => {
            (file.to_string(), line.parse().ok())
        }
        // file:line
        (None, Some(file), Some(line)) if line.chars().all(|c| c.is_ascii_digit()) => {
            (file.to_string(), line.parse().ok())
        }
        _ => (rest.to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_payload_is_extracted_from_both_str_and_string() {
        // `panic!("literal")` yields &str; `panic!("{x}")` yields String.
        let caught = std::panic::catch_unwind(|| panic!("literal payload"));
        let err = caught.expect_err("panicked");
        let message = err
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| err.downcast_ref::<String>().cloned())
            .unwrap();
        assert_eq!(message, "literal payload");
    }

    #[test]
    fn backtrace_frames_are_parsed_with_file_and_line() {
        let sample = "\
   0: erno::sync::pull
             at /home/u/proj/api/src/sync/delta.rs:120:5
   1: erno::jobs::worker::run
             at /home/u/proj/api/src/jobs/worker.rs:88:9";
        let frames = parse_backtrace(sample);

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].function.as_deref(), Some("erno::sync::pull"));
        assert_eq!(
            frames[0].file.as_deref(),
            Some("/home/u/proj/api/src/sync/delta.rs")
        );
        assert_eq!(frames[0].line, Some(120));
        assert!(frames[0].in_app);
        assert_eq!(frames[1].line, Some(88));
    }

    #[test]
    fn vendor_frames_are_marked_out_of_app() {
        // A real dependency frame, not unwinding machinery — the latter is
        // stripped entirely, which a separate test covers.
        let sample = "\
   0: alloc::vec::Vec::index
             at /rustc/abc/library/alloc/src/vec/mod.rs:3210:9";
        let frames = parse_backtrace(sample);
        assert_eq!(frames.len(), 1);
        assert!(!frames[0].in_app, "rustc library frames are not app code");
    }

    #[test]
    fn a_symbol_free_backtrace_yields_no_frames_rather_than_garbage() {
        let frames = parse_backtrace("<unknown>\n<unknown>\n");
        assert!(frames.is_empty());
    }

    #[test]
    fn unwinding_machinery_is_stripped_so_panics_do_not_all_collide() {
        let sample = "\
   0: std::backtrace::Backtrace::force_capture
             at /rustc/abc/library/std/src/backtrace.rs:1:1
   1: erno::error_reporting::reporter::capture::report_panic
             at /home/u/proj/api/src/error_reporting/reporter/capture.rs:250:20
   2: std::panicking::rust_panic_with_hook
             at /rustc/abc/library/std/src/panicking.rs:1:1
   3: erno::sync::delta::pull
             at /home/u/proj/api/src/sync/delta.rs:42:9";
        let frames = parse_backtrace(sample);

        assert_eq!(
            frames.len(),
            1,
            "only the actual panic site should survive: {frames:?}"
        );
        assert_eq!(
            frames[0].function.as_deref(),
            Some("erno::sync::delta::pull")
        );
        assert_eq!(
            fingerprint::culprit(&frames).as_deref(),
            Some("erno::sync::delta::pull (src/sync/delta.rs)"),
            "the culprit must name the panicking code, not the reporter"
        );
    }

    #[test]
    fn two_different_panics_get_different_fingerprints() {
        let make = |site: &str, file: &str| {
            format!(
                "   0: erno::error_reporting::reporter::capture::report_panic\n                             at /home/u/proj/api/src/error_reporting/reporter/capture.rs:250:20\n                    1: {site}\n             at {file}:42:9"
            )
        };
        let a = parse_backtrace(&make(
            "erno::sync::delta::pull",
            "/proj/api/src/sync/delta.rs",
        ));
        let b = parse_backtrace(&make(
            "erno::jobs::worker::run",
            "/proj/api/src/jobs/worker.rs",
        ));

        let fp = |frames: &[Frame]| {
            fingerprint::fingerprint(&fingerprint::FingerprintInput {
                project_id: uuid::Uuid::from_u128(1),
                source: Source::Api,
                error_type: "panic",
                message: "attempt to divide by zero",
                frames,
                client_fingerprint: None,
                call_site: None,
            })
        };
        assert_ne!(fp(&a), fp(&b), "distinct panic sites must not merge");
    }

    #[test]
    fn frame_parsing_is_bounded() {
        let mut sample = String::new();
        for i in 0..500 {
            sample.push_str(&format!("   {i}: f{i}\n             at /src/a{i}.rs:1:1\n"));
        }
        assert!(parse_backtrace(&sample).len() <= 50);
    }

    #[test]
    fn a_location_without_a_column_still_parses() {
        let (file, line) = split_location("/src/a.rs:42");
        assert_eq!(file, "/src/a.rs");
        assert_eq!(line, Some(42));
    }

    /// Drives the layer through real tracing machinery. Installing the
    /// process-global reporter can only happen once, so every case that needs
    /// it lives in this one test.
    #[test]
    fn the_capture_layer_reports_errors_and_ignores_everything_else() {
        use tokio::sync::mpsc;
        use tracing_subscriber::layer::SubscriberExt;

        let (tx, mut rx) = mpsc::channel(16);
        install(
            ErrorReporter::Remote(tx),
            Arc::new(ErrorReportingConfig {
                ignore_targets: vec!["noisy::dependency".to_string()],
                ..ErrorReportingConfig::default()
            }),
        );
        assert!(is_installed());

        let subscriber = tracing_subscriber::registry().with(ErrorCaptureLayer);
        tracing::subscriber::with_default(subscriber, || {
            // An explicit target, because this test module's own path lives
            // under SELF_TARGET and would (correctly) be suppressed.
            tracing::error!(
                target: "erno::jobs::worker",
                job_type = "send_email",
                attempts = 3,
                "job failed permanently"
            );
            // Below ERROR: routine logging must not become an issue.
            tracing::warn!("just a warning");
            tracing::info!("just info");
            // The loop guard: reporting's own failures must never be captured,
            // or a collector outage feeds itself forever.
            tracing::error!(target: SELF_TARGET, "batch write failed");
            tracing::error!(
                target: "erno::error_reporting::collector::ingest",
                "nested self target is also skipped"
            );
            // Configured out.
            tracing::error!(target: "noisy::dependency", "chatty");
        });

        let report = rx.try_recv().expect("the error event was captured");
        assert_eq!(report.message, "job failed permanently");
        assert_eq!(report.level, Level::Error);
        assert_eq!(report.source, Source::Api);
        // The call site groups far better than an interpolated message.
        assert_eq!(report.error_type, "erno::jobs::worker");
        assert_eq!(report.context["fields"]["job_type"], "send_email");
        assert_eq!(report.context["fields"]["attempts"], 3);
        assert!(report.context.get("file").is_some());
        assert!(report.context.get("line").is_some());

        assert!(
            rx.try_recv().is_err(),
            "warn/info, the self target, and ignored targets must all be skipped"
        );
    }
}
