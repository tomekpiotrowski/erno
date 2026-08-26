use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::ui;

const RING: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub label: String,
    pub line: String,
}

pub struct LogSink {
    verbose: bool,
    file: Option<Mutex<File>>,
    path: Option<PathBuf>,
    recent: Mutex<VecDeque<LogLine>>,
    capture_only: AtomicBool,
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
        if let Ok(mut q) = self.recent.lock() {
            if q.len() >= RING {
                q.pop_front();
            }
            q.push_back(LogLine {
                label: label.to_string(),
                line: line.to_string(),
            });
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
}
