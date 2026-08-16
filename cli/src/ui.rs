//! The one place the CLI writes to the terminal.
//!
//! Rendering is separated from printing: every `render_*` function is pure and
//! unit-tested, and the printing wrappers consult the flags `ui::init` stores
//! once in `main`. Nothing outside this file calls `println!`/`eprintln!` —
//! `tests/output_goes_through_ui.rs` enforces that.
//!
//! # The visual language
//!
//! ```text
//! ==> Section header              column 0, `==>` blue+bold, title bold
//!   ok    a row                   column 2, marker green/yellow/red
//!   warn  another row
//!   fail  a third row
//!         a continuation line     column 8, dim
//! error: something went wrong     column 0, `error:` red+bold
//! [api] a forwarded child line    column 0, `[api]` in the service colour
//! ```
//!
//! Markers are ASCII words, never emoji: `✅`/`❌` are East-Asian Wide while
//! `⚠️`/`ℹ️` are one column plus a variation selector, so no single pad count
//! aligns them on every terminal. Words are one column per character
//! everywhere, and they survive `--no-color` and piping — colour is decoration
//! only, and nothing is communicated by colour alone.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use anstyle::{AnsiColor, Color, Effects, Style};

// ── Global state, set once from main ─────────────────────────────────────────

static COLOR: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Called once from `main`, before any output. The values are immutable
/// afterwards, which is why they live here rather than being threaded through
/// every `handle_*` signature into closures that outlive their caller.
pub fn init(no_color: bool, quiet: bool, verbose: bool) {
    COLOR.store(resolve_color(no_color), Ordering::Relaxed);
    QUIET.store(quiet, Ordering::Relaxed);
    VERBOSE.store(verbose, Ordering::Relaxed);
}

pub fn color() -> bool {
    COLOR.load(Ordering::Relaxed)
}

pub fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// `--no-color` wins, then `NO_COLOR`, then `CLICOLOR_FORCE`, then "is stderr a
/// terminal that understands ANSI". Narration goes to stderr, so stderr is the
/// stream that decides — `erno build | less` keeps its colour.
fn resolve_color(no_color_flag: bool) -> bool {
    if no_color_flag || anstyle_query::no_color() {
        return false;
    }
    if anstyle_query::clicolor_force() {
        return true;
    }
    std::io::stderr().is_terminal() && anstyle_query::term_supports_color()
}

// ── Palette ──────────────────────────────────────────────────────────────────

const fn fg(c: AnsiColor) -> Style {
    Style::new().fg_color(Some(Color::Ansi(c)))
}

pub const GREEN: Style = fg(AnsiColor::Green);
pub const YELLOW: Style = fg(AnsiColor::Yellow);
pub const RED: Style = fg(AnsiColor::Red);
pub const CYAN: Style = fg(AnsiColor::Cyan);
pub const MAGENTA: Style = fg(AnsiColor::Magenta);
pub const BLUE: Style = fg(AnsiColor::Blue);
pub const DIM: Style = Style::new().effects(Effects::DIMMED);
pub const BOLD: Style = Style::new().effects(Effects::BOLD);
pub const HEADING: Style = fg(AnsiColor::Blue).effects(Effects::BOLD);
pub const ERROR: Style = fg(AnsiColor::Red).effects(Effects::BOLD);

/// Style `text`, honouring the global colour decision.
pub fn paint(style: Style, text: impl AsRef<str>) -> String {
    paint_when(color(), style, text.as_ref())
}

/// The pure form every `render_*` uses — and the one the tests exercise, so
/// they never touch global state and stay order-independent.
pub fn paint_when(on: bool, style: Style, text: &str) -> String {
    if !on {
        return text.to_string();
    }
    format!("{}{text}{}", style.render(), style.render_reset())
}

// ── Streams ──────────────────────────────────────────────────────────────────

/// stdout is the program's output; stderr is the program's narration.
///
/// Everything the CLI says *about* what it is doing goes to `Err`. Forwarded
/// subprocess output goes to whichever stream it came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stream {
    Out,
    Err,
}

pub fn emit(stream: Stream, line: &str) {
    match stream {
        Stream::Out => {
            let mut o = std::io::stdout().lock();
            let _ = writeln!(o, "{line}");
        }
        Stream::Err => {
            let mut e = std::io::stderr().lock();
            let _ = writeln!(e, "{line}");
        }
    }
}

fn narrate(line: &str) {
    emit(Stream::Err, line);
}

// ── The visual language ──────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Ok,
    Warn,
    Fail,
    /// A row with no marker — its text is dimmed and aligned with the rest.
    Info,
}

impl Level {
    fn word(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Fail => "fail",
            Level::Info => "",
        }
    }

    fn style(self) -> Style {
        match self {
            Level::Ok => GREEN,
            Level::Warn => YELLOW,
            Level::Fail => RED,
            Level::Info => DIM,
        }
    }
}

/// The only indent constants in the CLI. Rows sit at [`INDENT`], their
/// continuations at [`CONTINUATION`], headers and fatal errors at column 0.
pub const INDENT: &str = "  ";
/// Width of the marker field — `warn` and `fail` are the widest words.
const MARKER_WIDTH: usize = 4;
const GAP: usize = 2;
/// Continuation lines align under a row's text: `INDENT + MARKER_WIDTH + GAP`.
pub const CONTINUATION: &str = "        ";

pub fn render_row(on: bool, level: Level, text: &str) -> String {
    let word = level.word();
    let marker = if word.is_empty() {
        " ".repeat(MARKER_WIDTH)
    } else {
        paint_when(on, level.style(), &format!("{word:<MARKER_WIDTH$}"))
    };
    let body = if level == Level::Info {
        paint_when(on, DIM, text)
    } else {
        text.to_string()
    };
    format!("{INDENT}{marker}{}{body}", " ".repeat(GAP))
}

pub fn ok(text: impl AsRef<str>) {
    if !quiet() {
        narrate(&render_row(color(), Level::Ok, text.as_ref()));
    }
}

pub fn info(text: impl AsRef<str>) {
    if !quiet() {
        narrate(&render_row(color(), Level::Info, text.as_ref()));
    }
}

/// Warnings and failures are never suppressed by `--quiet`.
pub fn warn(text: impl AsRef<str>) {
    narrate(&render_row(color(), Level::Warn, text.as_ref()));
}

/// Explanation or remediation under the preceding row. Multi-line input is
/// split here so callers never hand-indent with `\n      `.
pub fn detail(text: impl AsRef<str>) {
    if quiet() {
        return;
    }
    for line in text.as_ref().lines() {
        narrate(&format!("{CONTINUATION}{}", paint(DIM, line.trim_start())));
    }
}

pub fn section(title: impl AsRef<str>) {
    if quiet() {
        return;
    }
    narrate("");
    narrate(&format!(
        "{} {}",
        paint(HEADING, "==>"),
        paint(BOLD, title.as_ref())
    ));
}

pub fn blank() {
    if !quiet() {
        narrate("");
    }
}

pub fn render_fatal(on: bool, message: &str) -> String {
    let mut lines = message.lines();
    let first = lines.next().unwrap_or_default();
    let mut out = format!("{} {first}", paint_when(on, ERROR, "error:"));
    for hint in lines {
        out.push('\n');
        out.push_str(&format!(
            "{INDENT}{}",
            paint_when(on, DIM, hint.trim_start())
        ));
    }
    out
}

/// The one fatal-error renderer. Line 1 becomes `error: …`; the rest are hints.
pub fn fatal(message: &str) {
    narrate(&render_fatal(color(), message));
}

// ── Row model ────────────────────────────────────────────────────────────────

/// A labelled result: `doctor`'s checks, `new`'s databases, `deploy`'s files.
///
/// The label column is sized from the widest label at render time, so no caller
/// hardcodes a width.
#[derive(Clone)]
pub struct Row {
    pub level: Level,
    pub label: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
}

impl Row {
    pub fn ok(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            level: Level::Ok,
            label: label.into(),
            detail: Some(detail.into()),
            hint: None,
        }
    }

    pub fn warn(label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            level: Level::Warn,
            label: label.into(),
            detail: None,
            hint: Some(hint.into()),
        }
    }

    pub fn fail(label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            level: Level::Fail,
            label: label.into(),
            detail: None,
            hint: Some(hint.into()),
        }
    }
}

pub fn render_rows(on: bool, rows: &[Row]) -> String {
    let width = column_width(rows.iter().map(|r| r.label.as_str()));
    let mut out = String::new();
    for row in rows {
        let text = match row.detail.as_deref().filter(|d| !d.is_empty()) {
            Some(detail) => format!("{:<width$}{}{detail}", row.label, " ".repeat(GAP)),
            None => row.label.clone(),
        };
        out.push_str(&render_row(on, row.level, &text));
        out.push('\n');
        if let Some(hint) = &row.hint {
            for line in hint.lines() {
                out.push_str(&format!(
                    "{CONTINUATION}{}\n",
                    paint_when(on, DIM, line.trim_start())
                ));
            }
        }
    }
    out
}

/// Print a report. Under `--quiet` only the rows that need attention survive,
/// so `erno doctor -q` shows just the problems.
pub fn print_rows(rows: &[Row]) {
    let shown: Vec<Row> = if quiet() {
        rows.iter()
            .filter(|r| matches!(r.level, Level::Warn | Level::Fail))
            .cloned()
            .collect()
    } else {
        rows.to_vec()
    };
    for line in render_rows(color(), &shown).lines() {
        narrate(line);
    }
}

// ── Subprocess prefixes ──────────────────────────────────────────────────────

/// Which colour a child's `[label]` prefix wears. Derived from the label so no
/// caller has to thread a colour through its call chain.
pub fn label_style(label: &str) -> Style {
    match label {
        "api" => CYAN,
        "app" => GREEN,
        "www" | "mail" => MAGENTA,
        "prom" | "admin" => YELLOW,
        _ => BLUE,
    }
}

pub fn render_prefixed(on: bool, label: &str, line: &str) -> String {
    format!(
        "{} {line}",
        paint_when(on, label_style(label), &format!("[{label}]"))
    )
}

pub fn prefixed(stream: Stream, label: &str, line: &str) {
    emit(stream, &render_prefixed(color(), label, line));
}

// ── Child process environment ────────────────────────────────────────────────

/// `std::process::Command` and `tokio::process::Command` share no trait, and
/// `packages.rs` is blocking while `dev/` is async. The *policy* lives here
/// once; each command type gets a two-line impl.
pub trait ChildCommand {
    fn set_env(&mut self, key: &str, value: &str);
}

impl ChildCommand for std::process::Command {
    fn set_env(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }
}

impl ChildCommand for tokio::process::Command {
    fn set_env(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }
}

/// Children see a pipe rather than a TTY, so tell them what we decided about
/// colour — including when we decided against it.
pub fn apply_child_env<C: ChildCommand>(cmd: &mut C) {
    if color() {
        for (k, v) in [
            ("CARGO_TERM_COLOR", "always"),
            ("FORCE_COLOR", "1"),
            ("CLICOLOR_FORCE", "1"),
            ("npm_config_color", "always"),
        ] {
            cmd.set_env(k, v);
        }
        if std::env::var_os("TERM").is_none() {
            cmd.set_env("TERM", "xterm-256color");
        }
    } else {
        for (k, v) in [
            ("CARGO_TERM_COLOR", "never"),
            ("NO_COLOR", "1"),
            ("FORCE_COLOR", "0"),
            ("npm_config_color", "false"),
        ] {
            cmd.set_env(k, v);
        }
    }
}

// ── Layout ───────────────────────────────────────────────────────────────────

/// Widest of `items`, for `{:<width$}`. Replaces every hardcoded column width.
pub fn column_width<'a>(items: impl IntoIterator<Item = &'a str>) -> usize {
    items
        .into_iter()
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(0)
}

/// Drop ANSI escape sequences, for matching and width maths against text a
/// child produced. Sixteen lines is cheaper than a crate.
pub fn strip_ansi(s: &str) -> String {
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

// ── Input ────────────────────────────────────────────────────────────────────

/// Prompts are narration, so they go to stderr like everything else.
pub fn prompt(label: &str, default: &str) -> String {
    let mut e = std::io::stderr().lock();
    let _ = write!(e, "{label}: ");
    let _ = e.flush();
    drop(e);

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return default.to_string();
    }
    let trimmed = input.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Ask a yes/no question. The `[Y/n]` / `[y/N]` suffix is added here so no
/// caller builds it. A closed or unreadable stdin takes the default.
pub fn confirm(question: &str, default_yes: bool) -> bool {
    let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };
    let mut e = std::io::stderr().lock();
    let _ = write!(e, "{question} {suffix} ");
    let _ = e.flush();
    drop(e);

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return default_yes;
    }
    match input.trim().to_ascii_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        "n" | "no" => false,
        // Unrecognised input takes the default rather than the safe answer,
        // matching the behaviour this replaced.
        _ => default_yes,
    }
}

// ── Command failure ──────────────────────────────────────────────────────────

/// A command's failure.
///
/// `Silent` means the command already reported the details — `run_phase` prints
/// a per-package `ok`/`fail` summary, so a second `error:` naming the same
/// package would be noise — and only the exit code is left to communicate.
pub enum Failure {
    Message(String),
    Silent,
}

impl From<String> for Failure {
    fn from(s: String) -> Self {
        Failure::Message(s)
    }
}

impl From<&str> for Failure {
    fn from(s: &str) -> Self {
        Failure::Message(s.to_string())
    }
}

/// What every `handle_*` returns. `From<String>` means the existing
/// `Result<_, String>` helpers work with a bare `?`.
pub type Cmd = std::result::Result<(), Failure>;

/// Render a fatal error and exit.
///
/// Prefer returning [`Failure`] — it unwinds, so guards like `DevLock` still
/// run their `Drop`. This exists for the scaffolding helpers in `new` and
/// `deploy`, which are called from deep inside `write`-style call chains where
/// there is nothing to clean up and threading a `Result` through every caller
/// would obscure more than it fixes. It routes through [`fatal`] so the message
/// looks identical either way.
pub fn abort(message: &str) -> ! {
    fatal(message);
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Colour is off in these tests by construction: `init` is never called, so
    // COLOR stays false. The `_when` variants are tested explicitly for both.

    #[test]
    fn paint_when_off_is_the_bare_text() {
        assert_eq!(paint_when(false, GREEN, "ok"), "ok");
    }

    #[test]
    fn paint_when_on_wraps_and_resets() {
        let painted = paint_when(true, GREEN, "ok");
        assert!(painted.contains("\u{1b}["));
        assert_ne!(painted, "ok");
        assert_eq!(strip_ansi(&painted), "ok");
    }

    #[test]
    fn rows_share_one_text_column() {
        assert_eq!(render_row(false, Level::Ok, "Rust"), "  ok    Rust");
        assert_eq!(render_row(false, Level::Warn, "Rust"), "  warn  Rust");
        assert_eq!(render_row(false, Level::Fail, "Rust"), "  fail  Rust");
        assert_eq!(render_row(false, Level::Info, "Rust"), "        Rust");
    }

    #[test]
    fn coloured_rows_align_with_uncoloured_ones() {
        for level in [Level::Ok, Level::Warn, Level::Fail, Level::Info] {
            assert_eq!(
                strip_ansi(&render_row(true, level, "Rust")),
                render_row(false, level, "Rust"),
            );
        }
    }

    #[test]
    fn continuation_lines_up_with_row_text() {
        assert_eq!(CONTINUATION.len(), INDENT.len() + MARKER_WIDTH + GAP);
        let row = render_row(false, Level::Fail, "x");
        assert_eq!(row.find('x'), Some(CONTINUATION.len()));
    }

    #[test]
    fn render_rows_sizes_the_label_column_from_the_widest_label() {
        let rows = vec![
            Row::ok("Rust", "1.90.0"),
            Row::ok("PostgreSQL client", "16.3"),
            Row::fail("psql", "not found\nInstall it"),
        ];
        let text = render_rows(false, &rows);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("  ok    Rust "));
        assert!(lines[1].starts_with("  ok    PostgreSQL client  "));
        assert_eq!(lines[2], "  fail  psql");
        assert_eq!(lines[3], "        not found");
        assert_eq!(lines[4], "        Install it");

        // The detail column starts at the same offset on every row: the widest
        // label ("PostgreSQL client") plus the gap.
        assert_eq!(lines[0].find("1.90.0"), lines[1].find("16.3"));
        assert_eq!(
            lines[0].find("1.90.0"),
            Some(CONTINUATION.len() + "PostgreSQL client".len() + GAP),
        );
    }

    #[test]
    fn render_fatal_puts_hints_under_the_message() {
        assert_eq!(
            render_fatal(false, "1 required check failed\nFix them and retry."),
            "error: 1 required check failed\n  Fix them and retry.",
        );
        assert_eq!(render_fatal(false, "boom"), "error: boom");
    }

    #[test]
    fn prefixed_lines_keep_the_child_text_verbatim() {
        assert_eq!(render_prefixed(false, "api", "hello"), "[api] hello");
        assert_eq!(
            strip_ansi(&render_prefixed(true, "api", "hello")),
            "[api] hello",
        );
    }

    #[test]
    fn label_styles_are_stable_and_fall_back() {
        assert_eq!(label_style("api"), CYAN);
        assert_eq!(label_style("app"), GREEN);
        assert_eq!(label_style("mail"), MAGENTA);
        assert_eq!(label_style("puzzles"), BLUE);
    }

    #[test]
    fn column_width_measures_characters_not_bytes() {
        assert_eq!(column_width(["a", "bbb"]), 3);
        assert_eq!(column_width([]), 0);
        assert_eq!(column_width(["café"]), 4);
    }

    #[test]
    fn strip_ansi_removes_escapes_and_keeps_text() {
        assert_eq!(strip_ansi("\u{1b}[31mERROR\u{1b}[0m boom"), "ERROR boom");
        assert_eq!(strip_ansi("plain"), "plain");
    }
}
