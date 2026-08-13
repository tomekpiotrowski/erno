use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct LogSink {
    verbose: bool,
    file: Option<Mutex<File>>,
    path: Option<PathBuf>,
}

impl LogSink {
    pub fn new(root: &Path, verbose: bool) -> Self {
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
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn write_line(&self, color: &str, label: &str, line: &str) {
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                let _ = writeln!(f, "[{label}] {line}");
            }
        }
        if should_print_line(line, self.verbose) {
            println!("{color}[{label}]{RESET} {line}", RESET = super::RESET);
        }
    }
}

pub fn should_print_line(line: &str, verbose: bool) -> bool {
    verbose || is_error_line(line) || is_notable_line(line)
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn is_error_line(line: &str) -> bool {
    let plain = strip_ansi(line);
    let lower = plain.to_ascii_lowercase();
    lower.contains("error")
        || lower.contains("failed")
        || lower.contains("panic")
        || plain.contains('✖')
        || plain.contains("error[E")
}

fn is_notable_line(line: &str) -> bool {
    let plain = strip_ansi(line);
    plain.contains("Local:")
        || plain.contains("Database is ready")
        || plain.contains("Server starting")
        || plain.contains("compiled successfully")
        || plain.contains("Application bundle generation complete")
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
    fn ready_events_print_in_quiet_mode() {
        assert!(should_print_line(
            "  Local:   http://localhost:4200/",
            false
        ));
        assert!(should_print_line("✅ Database is ready!", false));
        assert!(should_print_line(
            "🌐 Server starting on http://0.0.0.0:3000",
            false
        ));
    }
}
