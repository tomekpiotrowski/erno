use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::ui;

const RING: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub label: String,
    pub line: String,
    pub seq: u64,
}

#[cfg(test)]
impl LogLine {
    pub fn new(label: impl Into<String>, line: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            line: line.into(),
            seq: 0,
        }
    }
}

pub struct LogSink {
    verbose: bool,
    file: Option<Mutex<File>>,
    path: Option<PathBuf>,
    recent: Mutex<VecDeque<LogLine>>,
    capture_only: AtomicBool,
    seq: AtomicU64,
}

impl LogSink {
    pub fn new(root: &Path) -> Self {
        let verbose = ui::verbose();
        let dir = root.join(".erno");
        let path = dir.join("dev.log");
        let file = fs::create_dir_all(&dir)
            .ok()
            .and_then(|_| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .ok()
            })
            .map(Mutex::new);
        let opened = file.is_some();
        Self {
            verbose,
            file,
            path: opened.then_some(path),
            recent: Mutex::new(VecDeque::new()),
            capture_only: AtomicBool::new(false),
            seq: AtomicU64::new(0),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The TUI owns the screen: keep writing the file and the ring, but do not
    /// print. Prompts and errors still land in the log pane.
    pub fn set_capture_only(&self, yes: bool) {
        self.capture_only.store(yes, Ordering::Relaxed);
    }

    pub fn recent(&self) -> Vec<LogLine> {
        self.recent
            .lock()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn write_line(&self, stream: ui::Stream, label: &str, line: &str) {
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                // The file copy stays uncoloured so it greps cleanly.
                let _ = writeln!(f, "[{label}] {line}");
            }
        }
        // Readiness polls stay in the file; they must not fill the TUI ring
        // (or the WIRE pane, which is the same stream).
        if !is_probe_line(line) {
            if let Ok(mut q) = self.recent.lock() {
                if q.len() >= RING {
                    q.pop_front();
                }
                q.push_back(LogLine {
                    label: label.to_string(),
                    line: line.to_string(),
                    seq: self.seq.fetch_add(1, Ordering::Relaxed) + 1,
                });
            }
        }
        if self.capture_only.load(Ordering::Relaxed) {
            return;
        }
        if should_print_line(line, self.verbose) {
            ui::prefixed(stream, label, line);
        }
    }
}

/// What a child says that is worth interrupting the banner for.
///
/// Only two things qualify: something went wrong, or something is waiting on an
/// answer. Progress and readiness do not — the banner has a row per service and
/// probes each one, so a forwarded "Local: http://localhost:4200/" or "Database
/// is ready" says what the row beneath it already says, a second time and in
/// the child's words. `--verbose` is there for anyone who wants the raw
/// multiplex, and `.erno/dev.log` has every line either way.
pub fn should_print_line(line: &str, verbose: bool) -> bool {
    verbose || is_error_line(line) || is_prompt_line(line)
}

/// A child waiting on stdin must never be invisible: quiet mode still forwards
/// anything shaped like a question, or `erno dev` looks like it hung.
fn is_prompt_line(line: &str) -> bool {
    let plain = ui::strip_ansi(line);
    let text = plain.trim();
    if text.is_empty() {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    // A leading `?` is the inquirer/Ionic prompt marker.
    text.starts_with('?')
        || text.ends_with('?')
        || ["(y)", "(y/n)", "[y/n]", "(yes)", "(use arrow keys)"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
}

// The `✖` here comes from the `api` crate's own output, not from this CLI —
// the CLI's own icons are a separate set, and text it only forwards is not
// ours to restyle. Changing this means changing `api/`.
pub fn is_error_line(line: &str) -> bool {
    let plain = ui::strip_ansi(line);
    let lower = plain.to_ascii_lowercase();
    lower.contains("error")
        || lower.contains("failed")
        || lower.contains("panic")
        || plain.contains('✖')
        || plain.contains("error[E")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireKind {
    Request { error: bool },
    Reload,
}

/// WIRE is the log stream: HTTP access, HMR, panics.
/// `None` is chatter (or erno's own probe). Loki/Tempo `level=error` idle
/// lines are not traffic.
pub fn classify_wire_line(line: &str) -> Option<WireKind> {
    let plain = ui::strip_ansi(line);
    if is_probe_line(&plain) {
        return None;
    }
    let lower = plain.to_ascii_lowercase();
    if is_reload_line(&lower) {
        return Some(WireKind::Reload);
    }
    if let Some(error) = http_event_error(&plain) {
        return Some(WireKind::Request { error });
    }
    if is_panic_line(&plain, &lower) {
        return Some(WireKind::Request { error: true });
    }
    None
}

/// erno's readiness polls. Logged by the child, not interesting as traffic.
pub fn is_probe_line(line: &str) -> bool {
    let plain = ui::strip_ansi(line);
    if let Some(hit) = parse_access_log(&plain) {
        return hit.is_probe();
    }
    extract_uri(&plain).is_some_and(is_health_path)
}

fn is_reload_line(lower: &str) -> bool {
    lower.contains("hmr")
        || lower.contains("reload")
        || lower.contains("rebuilt")
        || lower.contains("rebuilding")
        || lower.contains("changes detected")
}

fn is_panic_line(plain: &str, lower: &str) -> bool {
    lower.contains("panic") || plain.contains('✖') || plain.contains("error[E")
}

fn http_event_error(plain: &str) -> Option<bool> {
    if let Some(hit) = parse_access_log(plain) {
        return Some(hit.is_error());
    }
    parse_tower_finished(plain)
}

fn is_health_path(uri: &str) -> bool {
    let path = uri.split('?').next().unwrap_or(uri);
    matches!(
        path,
        "/readiness" | "/liveness" | "/metrics" | "/-/ready" | "/ready"
    )
}

fn extract_uri(plain: &str) -> Option<&str> {
    let rest = plain.split("uri=").nth(1)?;
    Some(rest.split_whitespace().next().unwrap_or(rest))
}

struct AccessHit {
    status: u16,
    method: Option<String>,
    duration_ms: f64,
}

impl AccessHit {
    fn is_probe(&self) -> bool {
        matches!(self.method.as_deref(), Some("HEAD" | "OPTIONS"))
    }

    fn is_error(&self) -> bool {
        self.status >= 500 || self.duration_ms > 500.0
    }
}

/// Astro: `22:41:58 [200] /path 2ms`. API: `22:41:58.03 DEBUG [200] GET /api/x 4ms`.
fn parse_access_log(line: &str) -> Option<AccessHit> {
    let start = line.find('[')?;
    let rest = &line[start..];
    let close = rest.find(']')?;
    let status: u16 = rest[1..close].parse().ok()?;
    let mut rest = rest[close + 1..].trim();
    let mut method = None;
    for m in ["HEAD", "OPTIONS", "PATCH", "DELETE", "POST", "PUT", "GET"] {
        if rest.starts_with(m)
            && rest
                .as_bytes()
                .get(m.len())
                .is_some_and(|b| b.is_ascii_whitespace())
        {
            method = Some(m.to_string());
            rest = rest[m.len()..].trim();
            break;
        }
    }
    let (path, dur) = rest.rsplit_once(char::is_whitespace)?;
    if !path.starts_with('/') {
        return None;
    }
    let duration_ms = dur.strip_suffix("ms")?.parse().ok()?;
    Some(AccessHit {
        status,
        method,
        duration_ms,
    })
}

/// tower-http: `finished processing request latency=3 ms status=200 method=GET uri=/api/x`
fn parse_tower_finished(plain: &str) -> Option<bool> {
    if !plain.contains("finished processing request") {
        return None;
    }
    let status = field_after(plain, "status=")?.parse::<u16>().ok()?;
    let uri = extract_uri(plain)?;
    if is_health_path(uri) {
        return None;
    }
    let latency = field_after(plain, "latency=")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    Some(status >= 500 || latency > 500.0)
}

fn field_after<'a>(plain: &'a str, key: &str) -> Option<&'a str> {
    let rest = plain.split(key).nth(1)?;
    Some(rest.split_whitespace().next().unwrap_or(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbose_prints_everything() {
        assert!(should_print_line("hello", true));
        assert!(!should_print_line("hello", false));
    }

    #[test]
    fn errors_print_in_quiet_mode() {
        assert!(should_print_line("error: cannot find type", false));
        assert!(should_print_line("error[E0433]: failed to resolve", false));
        assert!(should_print_line("thread panicked at foo.rs", false));
        assert!(should_print_line("\u{1b}[31mERROR\u{1b}[0m boom", false));
    }

    #[test]
    fn capture_only_keeps_the_ring_and_stays_quiet() {
        let dir = std::env::temp_dir().join(format!("erno-log-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let sink = LogSink::new(&dir);
        sink.set_capture_only(true);
        sink.write_line(ui::Stream::Err, "api", "hello");
        sink.write_line(ui::Stream::Err, "api", "error: boom");
        let recent = sink.recent();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].label, "api");
        assert_eq!(recent[1].line, "error: boom");
        assert_eq!(recent[0].seq, 1);
        assert_eq!(recent[1].seq, 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompts_print_in_quiet_mode() {
        assert!(should_print_line("Ok to proceed? (y) ", false));
        assert!(should_print_line(
            "? Which device would you like to target: (Use arrow keys)",
            false
        ));
        assert!(should_print_line("Overwrite the file [y/N]", false));
        assert!(!should_print_line("Building app...", false));
    }

    #[test]
    fn readiness_chatter_stays_in_the_log_file() {
        // The banner says all of this, per service, and keeps saying it.
        for line in [
            "  Local:   http://localhost:4200/",
            "✅ Database is ready!",
            "🌐 Server starting on http://0.0.0.0:3000",
            "Application bundle generation complete. [1.688 seconds]",
            "compiled successfully",
        ] {
            assert!(!should_print_line(line, false), "{line}");
            assert!(should_print_line(line, true), "{line}");
        }
    }

    #[test]
    fn probes_are_not_enqueued_but_still_reach_the_file() {
        let dir = std::env::temp_dir().join(format!("erno-log-probe-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let sink = LogSink::new(&dir);
        sink.write_line(ui::Stream::Err, "www", "22:41:58 [200] HEAD / 2ms");
        sink.write_line(
            ui::Stream::Err,
            "api",
            "request: started processing request method=GET uri=/readiness version=HTTP/1.1",
        );
        sink.write_line(ui::Stream::Err, "www", "22:41:59 [200] / 3ms");
        let recent = sink.recent();
        assert_eq!(recent.len(), 1);
        assert!(recent[0].line.contains("[200] / 3ms"));
        let file = std::fs::read_to_string(dir.join(".erno").join("dev.log")).unwrap();
        assert!(file.contains("HEAD /"));
        assert!(file.contains("uri=/readiness"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn classify_wire_line_reads_http_hmr_and_skips_probes() {
        assert_eq!(
            classify_wire_line("22:41:58 [200] / 2ms"),
            Some(WireKind::Request { error: false })
        );
        assert_eq!(
            classify_wire_line("22:42:02 [500] /broken 12ms"),
            Some(WireKind::Request { error: true })
        );
        assert_eq!(classify_wire_line("22:41:58 [200] HEAD / 2ms"), None);
        assert_eq!(
            classify_wire_line("23:17:00.03 DEBUG [200] GET /api/projects 4ms"),
            Some(WireKind::Request { error: false })
        );
        assert_eq!(
            classify_wire_line(
                "request: finished processing request latency=3 ms status=200 method=GET uri=/api/projects version=HTTP/1.1"
            ),
            Some(WireKind::Request { error: false })
        );
        assert_eq!(
            classify_wire_line(
                "request: finished processing request latency=1 ms status=200 method=GET uri=/readiness version=HTTP/1.1"
            ),
            None
        );
        assert_eq!(
            classify_wire_line("Page reload sent to client"),
            Some(WireKind::Reload)
        );
        assert_eq!(
            classify_wire_line("✔ Changes detected. Rebuilding..."),
            Some(WireKind::Reload)
        );
        assert_eq!(
            classify_wire_line("Worker 'default-0' received job notification"),
            None
        );
        assert_eq!(
            classify_wire_line(
                r#"level=error msg="error processing requests from scheduler" err="context canceled""#
            ),
            None
        );
    }
}
