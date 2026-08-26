//! The one place the CLI writes to the terminal.
//!
//! Rendering is separated from printing: every `render_*` function is pure and
//! unit-tested, and the printing wrappers consult the flags `ui::init` stores
//! once in `main`. Nothing outside this file calls `println!`/`eprintln!` —
//! `tests/output_goes_through_ui.rs` enforces that.
//!
//! # The visual language
//!
//! Two faces, one geometry. [`Face`] carries the two decisions — colour and
//! emoji — and every `render_*` takes one:
//!
//! ```text
//!  emoji face                      ascii face
//!
//! 🩺 Section header                ==> Section header
//!   ✅    a row                      ok    a row
//!   ⚠️    another row                warn  another row
//!   ❌    a third row                fail  a third row
//!         a continuation line              a continuation line
//! ❌ error: something went wrong    error: something went wrong
//! [api] a forwarded child line     [api] a forwarded child line
//! ```
//!
//! Row text sits at column 8 in both, because an icon is padded to the same
//! marker width as the widest word (`warn`/`fail`). That only holds while every
//! icon is exactly two columns wide, so the icons are a curated set in [`icon`]
//! and a unit test measures every one of them with [`display_width`].
//!
//! The ASCII face is the fallback, and it engages wherever emoji are a gamble:
//! `--no-emoji`, `ERNO_EMOJI=0`, and anywhere colour is already off — pipes,
//! CI, `--no-color`, `NO_COLOR`, a terminal that does not do ANSI. Colour and
//! emoji are decoration only; nothing is communicated by either alone.
//!
//! # The pinned region
//!
//! `erno dev` pins its status banner to the bottom of the terminal and redraws
//! it in place. That is the one piece of cursor control the CLI owns, it lives
//! entirely in this file, and it is stderr-only. Every write — both streams —
//! goes through one mutex so erase, write, and redraw cannot interleave. When
//! the terminal cannot support it, [`pin`] returns `None` and callers print the
//! scrolling way instead; that fallback is the behaviour the CLI has always
//! had, so pipes, CI, `--no-color`, `--quiet`, `--verbose`, and Windows are all
//! untouched by any of this.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use anstyle::{AnsiColor, Color, Effects, Style};

// ── Global state, set once from main ─────────────────────────────────────────

static COLOR: AtomicBool = AtomicBool::new(false);
static EMOJI: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);
/// The pinned region lives on stderr, so a stdout write only has to dodge it
/// when both land on the same screen.
static STDOUT_TTY: AtomicBool = AtomicBool::new(false);
/// Called from [`fatal`] so the TUI can leave alt-screen even when we `exit`
/// without unwinding (child spawn failures).
static FATAL_HOOK: Mutex<Option<fn()>> = Mutex::new(None);
/// When the TUI owns the screen, narration on stderr would scramble alt-screen.
static TUI_LIVE: AtomicBool = AtomicBool::new(false);

/// Called once from `main`, before any output. The values are immutable
/// afterwards, which is why they live here rather than being threaded through
/// every `handle_*` signature into closures that outlive their caller.
pub fn init(no_color: bool, no_emoji: bool, quiet: bool, verbose: bool) {
    let color = resolve_color(no_color);
    COLOR.store(color, Ordering::Relaxed);
    EMOJI.store(resolve_emoji(no_emoji, color), Ordering::Relaxed);
    QUIET.store(quiet, Ordering::Relaxed);
    VERBOSE.store(verbose, Ordering::Relaxed);
    STDOUT_TTY.store(std::io::stdout().is_terminal(), Ordering::Relaxed);
}

pub fn color() -> bool {
    COLOR.load(Ordering::Relaxed)
}

pub fn emoji() -> bool {
    EMOJI.load(Ordering::Relaxed)
}

pub fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Install a hook that [`fatal`] runs before printing. The TUI uses this to
/// leave the alternate screen when a child spawn calls `exit` without dropping
/// its guard. Pass `None` to clear.
pub fn set_fatal_hook(hook: Option<fn()>) {
    *FATAL_HOOK.lock().unwrap_or_else(PoisonError::into_inner) = hook;
}

/// Suppress `emit` / `prefixed` / `info` while the dashboard holds alt-screen.
pub fn set_tui_live(live: bool) {
    TUI_LIVE.store(live, Ordering::Relaxed);
}

pub fn tui_live() -> bool {
    TUI_LIVE.load(Ordering::Relaxed)
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

/// `--no-emoji` wins, then `ERNO_EMOJI=0`, then the colour decision.
///
/// Riding on `color` is deliberate rather than lazy: the things that turn
/// colour off — a pipe, a redirect, CI, `NO_COLOR`, a terminal that does not do
/// ANSI — are the same things that make a two-column glyph a gamble. One switch
/// covers both, and `ERNO_EMOJI=0` is there for a colour terminal with a font
/// that cannot draw them.
fn resolve_emoji(no_emoji_flag: bool, color: bool) -> bool {
    !no_emoji_flag && std::env::var("ERNO_EMOJI").as_deref() != Ok("0") && color
}

// ── The face ─────────────────────────────────────────────────────────────────

/// The two rendering decisions, together.
///
/// Every `render_*` takes one instead of a bare `on: bool`, so a call site
/// cannot silently swap the two flags — and so the pure renderers stay pure:
/// [`Face::PLAIN`] is what the tests pass, and nothing reads global state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Face {
    pub color: bool,
    pub emoji: bool,
}

impl Face {
    /// No colour, no emoji: the lowest common denominator, and the face every
    /// unit test renders with. Nothing at runtime picks a face by hand — that
    /// is what [`Face::current`] is for — so this exists for the tests only.
    #[cfg(test)]
    pub const PLAIN: Face = Face {
        color: false,
        emoji: false,
    };

    /// The face `ui::init` resolved for this run.
    pub fn current() -> Face {
        Face {
            color: color(),
            emoji: emoji(),
        }
    }
}

// ── Icons ────────────────────────────────────────────────────────────────────

/// Every emoji the CLI emits. One curated set, because the layout depends on
/// knowing each glyph's width.
///
/// The entry price is **exactly two columns**, which in practice means
/// `Emoji_Presentation=Yes` — a glyph that is wide on its own, with no
/// variation selector talking a terminal into it. `⚠️` is the one exception the
/// set makes, for a marker with no wide synonym that reads as "warning"; it is
/// written with its U+FE0F and, like the rest, measured by
/// `every_icon_is_two_columns_wide`.
///
/// `⚙️` and `🛠️` are absent for the same reason `⚠️` is a note: they are
/// text-presentation by default. `🦀` (api) and `🧰` (admin) stand in.
pub mod icon {
    // Row markers.
    pub const OK: &str = "✅";
    pub const WARN: &str = "⚠️";
    pub const FAIL: &str = "❌";

    // `dev` service states.
    pub const READY: &str = "✅";
    pub const STARTING: &str = "⏳";
    pub const MIGRATING: &str = "🔄";

    // Section headers, one per command.
    pub const DOCTOR: &str = "🩺";
    pub const NEW: &str = "✨";
    pub const SETUP: &str = "🔧";
    pub const DEV: &str = "🚀";
    pub const BUILD: &str = "🔨";
    pub const LINT: &str = "🧹";
    pub const TEST: &str = "🧪";
    pub const UPGRADE: &str = "⏫";
    pub const DEPLOY: &str = "🚢";

    // Section headers, one per topic.
    pub const PACKAGE: &str = "📦";
    pub const DATABASE: &str = "🗃️";
    pub const KEY: &str = "🔑";
    pub const CLOUD: &str = "☁️";
    pub const NOTE: &str = "📝";
    pub const DONE: &str = "🎉";

    /// Every constant above, for the width test and nothing else.
    #[cfg(test)]
    pub const ALL: [&str; 20] = [
        OK, WARN, FAIL, READY, STARTING, MIGRATING, DOCTOR, NEW, SETUP, DEV, BUILD, LINT, TEST,
        UPGRADE, DEPLOY, PACKAGE, DATABASE, KEY, CLOUD, NOTE,
    ];
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
    if tui_live() {
        return;
    }
    frame(&mut region(), stream, &[line]);
}

/// Write a multi-line render as one frame, under one lock.
///
/// The per-line alternative lets another task's `[api] …` land in the middle of
/// a banner or a row block, which it used to. Anything rendered as a block
/// should go out as one.
pub fn emit_block(stream: Stream, text: &str) {
    if tui_live() {
        return;
    }
    let lines: Vec<&str> = text.lines().collect();
    frame(&mut region(), stream, &lines);
}

fn narrate(line: &str) {
    emit(Stream::Err, line);
}

// ── The pinned region ────────────────────────────────────────────────────────

/// The region pinned to the bottom of the terminal: `erno dev`'s status banner,
/// redrawn in place while logs scroll above it.
///
/// The invariant everything here maintains: the region occupies the last
/// `drawn` rows of the screen and the cursor sits at column 0 of the row below
/// it. So a write emits exactly `body.len() + lines.len()` newlines, and
/// `drawn` is how far to walk back up next time.
struct Region {
    /// What the caller pinned, unfitted — refitting from this on every redraw
    /// is what makes a shrink-then-grow lossless.
    source: Vec<String>,
    /// `source` fitted to the current terminal. What is actually on screen.
    lines: Vec<String>,
    /// Rows the last frame drew: the cursor-up count.
    drawn: usize,
    active: bool,
    /// Last known terminal size, kept so a failed `ioctl` mid-session reuses it
    /// rather than dropping the region.
    size: Option<(usize, usize)>,
}

impl Region {
    const fn new() -> Self {
        Self {
            source: Vec::new(),
            lines: Vec::new(),
            drawn: 0,
            active: false,
            size: None,
        }
    }
}

/// The single output mutex: **every** write to either stream goes through it,
/// which is what makes erase → write → redraw atomic against the ~19 tasks that
/// print during `erno dev`.
///
/// Re-entrancy rule: nothing holding this lock may call a `ui` function that
/// takes it. There is one writer ([`frame`]) and every public entry point
/// acquires the guard exactly once.
static REGION: Mutex<Region> = Mutex::new(Region::new());

/// A panicking printer task must not silence the CLI, so poisoning is ignored.
fn region() -> MutexGuard<'static, Region> {
    REGION.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A live pinned region. Dropping it erases the region and leaves its final
/// contents in the scrollback as ordinary output.
///
/// Modelled on `dev::lock::DevLock`: the guard is what makes an early `?` safe.
pub struct Pinned(());

impl Drop for Pinned {
    fn drop(&mut self) {
        let mut r = region();
        if !r.active {
            return;
        }
        // Erase the live copy, then print it once more as scrolled output, so
        // the URLs survive the session instead of vanishing with the region.
        let last = std::mem::take(&mut r.lines);
        r.active = false;
        r.source.clear();
        let body: Vec<&str> = last.iter().map(String::as_str).collect();
        frame(&mut r, Stream::Err, &body);
    }
}

/// Pin `lines` to the bottom of the terminal.
///
/// `None` means this terminal cannot support it — the caller then prints the
/// old way and never calls [`repin`].
pub fn pin(lines: &[String]) -> Option<Pinned> {
    if !sticky_supported() {
        return None;
    }
    let mut r = region();
    let (cols, rows) = terminal_size()?;
    if !region_fits(lines, cols, rows) {
        return None;
    }
    r.source = lines.to_vec();
    r.size = Some((cols, rows));
    r.active = true;
    frame(&mut r, Stream::Err, &[]);
    Some(Pinned(()))
}

/// Replace the pinned content, redrawing in place. A no-op when nothing is
/// pinned — which is what makes the shutdown race benign: once the guard is
/// dropped the readiness watcher goes quiet on its own.
pub fn repin(lines: &[String]) {
    let mut r = region();
    if !r.active {
        return;
    }
    r.source = lines.to_vec();
    frame(&mut r, Stream::Err, &[]);
}

/// Take the region off the screen for good. Used before a fatal error, which
/// must be the last thing on screen, and which may be followed by a
/// non-unwinding `exit` that would otherwise strand the banner.
fn clear_region() {
    let mut r = region();
    if r.drawn > 0 {
        let mut e = std::io::stderr().lock();
        let _ = e.write_all(render_frame(r.drawn, &[], &[]).as_bytes());
        let _ = e.flush();
    }
    r.drawn = 0;
    r.active = false;
    r.lines.clear();
    r.source.clear();
}

/// A pinned region needs a unix terminal on stderr that understands ANSI, and a
/// caller who has not asked for something else.
///
/// `is_terminal` is checked separately from [`color`] on purpose:
/// `CLICOLOR_FORCE=1` makes colour true even when stderr is a file, and cursor
/// escapes must never reach a redirect. `--verbose` is excluded because it is
/// the raw multiplex — high-rate child output fights a pinned region.
fn sticky_supported() -> bool {
    cfg!(unix)
        && std::io::stderr().is_terminal()
        && color()
        && !quiet()
        && !verbose()
        && std::env::var("ERNO_STICKY").as_deref() != Ok("0")
}

/// The terminal's `(columns, rows)`, read from stderr — the stream the region
/// lives on. `None` when stderr is not a terminal or the call fails.
///
/// Re-read on every redraw rather than cached: `SIGWINCH` is not handled, and
/// one `ioctl` is nothing next to the write syscall it accompanies. A TTL cache
/// here would buy no measurable time and add a staleness bug.
#[cfg(unix)]
fn terminal_size() -> Option<(usize, usize)> {
    use std::os::unix::io::AsRawFd;

    // SAFETY: `ws` is a plain out-parameter; TIOCGWINSZ only writes into it.
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::ioctl(std::io::stderr().as_raw_fd(), libc::TIOCGWINSZ, &mut ws) };
    (rc == 0 && ws.ws_col > 0 && ws.ws_row > 0).then_some((ws.ws_col as usize, ws.ws_row as usize))
}

#[cfg(not(unix))]
fn terminal_size() -> Option<(usize, usize)> {
    None
}

fn refit(r: &mut Region) {
    if let Some(size) = terminal_size() {
        r.size = Some(size);
    }
    match r.size {
        Some((cols, rows)) => r.lines = fit_region(&r.source, cols, rows),
        None => r.lines.clear(),
    }
}

/// The one writer. Everything visible passes through here.
fn frame(r: &mut Region, stream: Stream, body: &[&str]) {
    if r.active {
        refit(r);
    } else {
        r.lines.clear();
    }

    // While a region is live, forwarded text must not move the cursor itself —
    // a child drawing its own progress would dislodge the count. Piped and
    // non-sticky output stays byte-for-byte verbatim.
    let owned: Vec<String> = if r.active {
        body.iter().map(|l| strip_cursor_control(l)).collect()
    } else {
        body.iter().map(|l| (*l).to_string()).collect()
    };
    let body: Vec<&str> = owned.iter().map(String::as_str).collect();

    match stream {
        Stream::Err => {
            let text = render_frame(r.drawn, &body, &r.lines);
            let mut e = std::io::stderr().lock();
            let _ = e.write_all(text.as_bytes());
            let _ = e.flush();
            r.drawn = r.lines.len();
        }
        Stream::Out => {
            // The region is on stderr, so a stdout line has to be sandwiched
            // between an erase and a redraw — but only when both streams share
            // a screen. Under `erno dev > out.txt` there is nothing to disturb,
            // and skipping the pair keeps the region from flickering on every
            // forwarded line.
            let sharing = r.active && STDOUT_TTY.load(Ordering::Relaxed);
            if sharing && r.drawn > 0 {
                let mut e = std::io::stderr().lock();
                let _ = e.write_all(render_frame(r.drawn, &[], &[]).as_bytes());
                let _ = e.flush();
                r.drawn = 0;
            }
            {
                // stdout is line-buffered where stderr is not, so it must be
                // flushed before the region is redrawn or the two interleave.
                let mut o = std::io::stdout().lock();
                for line in &body {
                    let _ = writeln!(o, "{line}");
                }
                let _ = o.flush();
            }
            if sharing {
                let mut e = std::io::stderr().lock();
                let _ = e.write_all(render_frame(0, &[], &r.lines).as_bytes());
                let _ = e.flush();
                r.drawn = r.lines.len();
            }
        }
    }
}

/// One frame: erase the `drawn` rows the last frame left, print `body`, then
/// redraw `region` beneath it.
///
/// The escape vocabulary is deliberately two sequences — cursor-up and
/// erase-to-end-of-display. No alternate screen, no raw mode, no cursor hiding,
/// so even a SIGKILL leaves the terminal in a normal state.
pub fn render_frame(drawn: usize, body: &[&str], region: &[String]) -> String {
    let mut out = String::new();
    if drawn > 0 {
        // `\r` is insurance against a `write!` that left the cursor mid-line.
        out.push_str(&format!("\r\u{1b}[{drawn}A\u{1b}[0J"));
    }
    for line in body
        .iter()
        .copied()
        .chain(region.iter().map(String::as_str))
    {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Cut `line` to `width` printable columns.
///
/// Escape sequences occupy no columns and are never split — the scanner walks
/// whole CSI sequences the way [`strip_ansi`] does, but keeps them. A line cut
/// mid-style gets an explicit reset so colour cannot bleed into the next row.
/// Columns are counted with [`char_width`], so a two-column glyph is dropped
/// whole rather than leaving half a cell for the terminal to guess at.
pub fn truncate_display(line: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut shown = 0;
    let mut styled = false;
    let mut cut = false;
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            styled = true;
            out.push(c);
            out.push('[');
            chars.next();
            for next in chars.by_ref() {
                out.push(next);
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        let w = if chars.peek() == Some(&EMOJI_PRESENTATION) {
            2
        } else {
            char_width(c)
        };
        if shown + w > width {
            cut = true;
            break;
        }
        out.push(c);
        shown += w;
    }
    if cut && styled {
        out.push_str("\u{1b}[0m");
    }
    out
}

/// Rows a pinned region leaves for the cursor and the logs above it.
const MIN_SCROLL_ROWS: usize = 4;

/// Whether this terminal is worth pinning to.
///
/// The bar is the whole region fitting as it is, with rows to spare for the
/// scrolling output above it: a region cut down to its first few rows, or wide
/// enough to lose the state column, tells you less than the scrolling banner
/// does. A terminal *resized* below the bar mid-session is treated more kindly —
/// [`fit_region`] just truncates — because that is a transient the user is
/// already watching.
pub fn region_fits(lines: &[String], cols: usize, rows: usize) -> bool {
    let widest = lines.iter().map(display_width).max().unwrap_or(0);
    !lines.is_empty() && widest < cols && rows >= lines.len() + MIN_SCROLL_ROWS
}

/// Fit the pinned lines to the terminal.
///
/// At most `rows - 1` lines — the spare row is where the cursor lives — each at
/// most `cols - 1` columns, the spare column dodging the deferred-wrap
/// ambiguity you get from writing exactly `cols` characters. A wrapped region
/// line would break the cursor-up count; wrapped *log* lines are harmless,
/// since those rows are never counted. An empty result means the terminal is
/// too small to pin anything, and the caller falls back to scrolling.
pub fn fit_region(lines: &[String], cols: usize, rows: usize) -> Vec<String> {
    if cols <= 2 || rows <= 2 {
        return Vec::new();
    }
    lines
        .iter()
        .take(rows - 1)
        .map(|l| truncate_display(l, cols - 1))
        .collect()
}

/// Drop carriage returns and every CSI sequence except SGR.
///
/// Colour is harmless; motion and erase are not. Applied only to forwarded text
/// while a region is live — the `.erno/dev.log` copy is written before this and
/// stays verbatim either way.
pub fn strip_cursor_control(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            continue;
        }
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            let mut seq = String::from("\u{1b}[");
            chars.next();
            for next in chars.by_ref() {
                seq.push(next);
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            if seq.ends_with('m') {
                out.push_str(&seq);
            }
            continue;
        }
        out.push(c);
    }
    out
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

    fn icon(self) -> Option<&'static str> {
        match self {
            Level::Ok => Some(icon::OK),
            Level::Warn => Some(icon::WARN),
            Level::Fail => Some(icon::FAIL),
            Level::Info => None,
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

/// The marker column, in either face.
///
/// Both are [`MARKER_WIDTH`] columns wide, which is what keeps row text at
/// [`CONTINUATION`] whichever face is in play. The emoji is left unpainted on
/// purpose: it arrives with its own colour, and an SGR wrapper only gives some
/// terminals licence to override it.
fn render_marker(face: Face, level: Level) -> String {
    if face.emoji {
        return match level.icon() {
            Some(glyph) => format!("{glyph}{}", " ".repeat(MARKER_WIDTH - display_width(glyph))),
            None => " ".repeat(MARKER_WIDTH),
        };
    }
    match level.word() {
        "" => " ".repeat(MARKER_WIDTH),
        word => paint_when(face.color, level.style(), &format!("{word:<MARKER_WIDTH$}")),
    }
}

pub fn render_row(face: Face, level: Level, text: &str) -> String {
    let body = if level == Level::Info {
        paint_when(face.color, DIM, text)
    } else {
        text.to_string()
    };
    format!(
        "{INDENT}{}{}{body}",
        render_marker(face, level),
        " ".repeat(GAP)
    )
}

pub fn ok(text: impl AsRef<str>) {
    if !quiet() {
        narrate(&render_row(Face::current(), Level::Ok, text.as_ref()));
    }
}

pub fn info(text: impl AsRef<str>) {
    if !quiet() {
        narrate(&render_row(Face::current(), Level::Info, text.as_ref()));
    }
}

/// Warnings and failures are never suppressed by `--quiet`.
pub fn warn(text: impl AsRef<str>) {
    narrate(&render_row(Face::current(), Level::Warn, text.as_ref()));
}

/// Explanation or remediation under the preceding row. Multi-line input is
/// split here so callers never hand-indent with `\n      `.
pub fn detail(text: impl AsRef<str>) {
    if quiet() {
        return;
    }
    let block: String = text
        .as_ref()
        .lines()
        .map(|line| format!("{CONTINUATION}{}\n", paint(DIM, line.trim_start())))
        .collect();
    emit_block(Stream::Err, &block);
}

/// A section header: `🩺 Environment`, or `==> Environment` without emoji.
///
/// The icon replaces the arrow rather than joining it — two lead-ins would put
/// the title at a different column in each face, and the blank line above is
/// what separates sections either way.
pub fn render_section(face: Face, icon: &str, title: &str) -> String {
    let lead = if face.emoji {
        icon.to_string()
    } else {
        paint_when(face.color, HEADING, "==>")
    };
    format!("\n{lead} {}", paint_when(face.color, BOLD, title))
}

pub fn section(icon: &str, title: impl AsRef<str>) {
    if quiet() {
        return;
    }
    emit_block(
        Stream::Err,
        &render_section(Face::current(), icon, title.as_ref()),
    );
}

/// A command's result: the one line that says how it went.
///
/// Not narration — this is what the command was run to find out — so it
/// survives `--quiet`, like the per-package summary in `run_phase`.
pub fn render_finished(face: Face, icon: &str, message: &str) -> String {
    if face.emoji {
        format!("{icon} {message}")
    } else {
        format!("{} {message}", paint_when(face.color, GREEN, "done:"))
    }
}

pub fn finished(icon: &str, message: impl AsRef<str>) {
    emit_block(
        Stream::Err,
        &render_finished(Face::current(), icon, message.as_ref()),
    );
}

/// A titled list of things to run next: `erno new`, `erno deploy init`.
///
/// The title is a row and the entries are its continuations, so this adds no
/// new indent — it is the same two levels every other block uses.
pub fn render_next_steps(face: Face, title: &str, steps: &[String]) -> String {
    let mut out = format!("{INDENT}{}\n", paint_when(face.color, BOLD, title));
    for step in steps {
        for line in step.lines() {
            out.push_str(&format!(
                "{CONTINUATION}{}\n",
                paint_when(face.color, CYAN, line)
            ));
        }
    }
    out
}

pub fn next_steps(title: &str, steps: &[String]) {
    if !quiet() {
        emit_block(
            Stream::Err,
            &render_next_steps(Face::current(), title, steps),
        );
    }
}

pub fn blank() {
    if !quiet() {
        narrate("");
    }
}

pub fn render_fatal(face: Face, message: &str) -> String {
    let mut lines = message.lines();
    let first = lines.next().unwrap_or_default();
    let lead = paint_when(face.color, ERROR, "error:");
    let mut out = if face.emoji {
        format!("{} {lead} {first}", icon::FAIL)
    } else {
        format!("{lead} {first}")
    };
    for hint in lines {
        out.push('\n');
        out.push_str(&format!(
            "{INDENT}{}",
            paint_when(face.color, DIM, hint.trim_start())
        ));
    }
    out
}

/// The one fatal-error renderer. Line 1 becomes `error: …`; the rest are hints.
///
/// The pinned region is taken down first: an error should be the last thing on
/// screen, and this is also the guard for the non-unwinding paths — `ui::abort`
/// and `dev::process`'s spawn failure both `exit` without dropping [`Pinned`].
pub fn fatal(message: &str) {
    if let Some(hook) = *FATAL_HOOK.lock().unwrap_or_else(PoisonError::into_inner) {
        hook();
    }
    clear_region();
    emit_block(Stream::Err, &render_fatal(Face::current(), message));
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

pub fn render_rows(face: Face, rows: &[Row]) -> String {
    let width = column_width(rows.iter().map(|r| r.label.as_str()));
    let mut out = String::new();
    for row in rows {
        let text = match row.detail.as_deref().filter(|d| !d.is_empty()) {
            Some(detail) => format!("{:<width$}{}{detail}", row.label, " ".repeat(GAP)),
            None => row.label.clone(),
        };
        out.push_str(&render_row(face, row.level, &text));
        out.push('\n');
        if let Some(hint) = &row.hint {
            for line in hint.lines() {
                out.push_str(&format!(
                    "{CONTINUATION}{}\n",
                    paint_when(face.color, DIM, line.trim_start())
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
    emit_block(Stream::Err, &render_rows(Face::current(), &shown));
}

// ── Subprocess prefixes ──────────────────────────────────────────────────────

/// Which colour a child's `[label]` prefix wears. Derived from the label so no
/// caller has to thread a colour through its call chain.
pub fn label_style(label: &str) -> Style {
    match label {
        "api" => CYAN,
        "app" => GREEN,
        "www" | "mail" => MAGENTA,
        "prom" | "mon" | "console" | "admin" => YELLOW,
        _ => BLUE,
    }
}

/// The icon a service wears in the `dev` banner. Paired with [`label_style`] —
/// same labels, same fallback — so a new service is two lines, not a hunt.
pub fn label_icon(label: &str) -> &'static str {
    match label {
        "api" => "🦀",
        "app" => "📱",
        "www" => "🌐",
        "mail" => "📧",
        "prom" => "📊",
        "mon" => "📡",
        "console" => "🔭",
        "admin" => "🧰",
        _ => "🔹",
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
    items.into_iter().map(display_width).max().unwrap_or(0)
}

/// The columns `s` occupies on screen. Escapes and combining characters take
/// none; emoji take two.
///
/// This is the measurement every column in the CLI is built on, so it has to
/// agree with the terminal about the icons in [`icon`] and about ASCII. It does
/// not have to be a general Unicode width implementation, and it is not one —
/// a crate for that would be several hundred kilobytes of tables to adjudicate
/// text this CLI never prints.
pub fn display_width(s: impl AsRef<str>) -> usize {
    let plain = strip_ansi(s.as_ref());
    let chars: Vec<char> = plain.chars().collect();
    (0..chars.len())
        .map(|i| {
            // U+FE0F asks for emoji presentation, and emoji presentation is two
            // columns whatever the base character would be on its own. This is
            // what makes `⚠️` measure the same as `✅`.
            if chars.get(i + 1) == Some(&EMOJI_PRESENTATION) {
                2
            } else {
                char_width(chars[i])
            }
        })
        .sum()
}

/// U+FE0F VARIATION SELECTOR-16: "draw the character before me as an emoji".
const EMOJI_PRESENTATION: char = '\u{FE0F}';

/// Columns for one `char`: 0 for a modifier, 2 for a wide glyph, 1 otherwise.
///
/// The wide set is the East-Asian Wide blocks plus the emoji planes, plus the
/// scattered `U+2xxx`/`U+3xxx` singles that carry `Emoji_Presentation=Yes` —
/// those are the ones that surprise you, because their neighbours in the same
/// block are one column.
fn char_width(c: char) -> usize {
    match c as u32 {
        // Zero-width: combining marks, the joiner, variation selectors, and the
        // skin-tone modifiers. Each attaches to the glyph before it.
        0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F | 0x1F3FB..=0x1F3FF => 0,
        // Emoji with a default emoji presentation, outside the emoji planes.
        0x231A..=0x231B
        | 0x23E9..=0x23EC
        | 0x23F0
        | 0x23F3
        | 0x25FD..=0x25FE
        | 0x2614..=0x2615
        | 0x2648..=0x2653
        | 0x267F
        | 0x2693
        | 0x26A1
        | 0x26AA..=0x26AB
        | 0x26BD..=0x26BE
        | 0x26C4..=0x26C5
        | 0x26CE
        | 0x26D4
        | 0x26EA
        | 0x26F2..=0x26F3
        | 0x26F5
        | 0x26FA
        | 0x26FD
        | 0x2705
        | 0x270A..=0x270B
        | 0x2728
        | 0x274C
        | 0x274E
        | 0x2753..=0x2755
        | 0x2757
        | 0x2795..=0x2797
        | 0x27B0
        | 0x27BF
        | 0x2B1B..=0x2B1C
        | 0x2B50
        | 0x2B55 => 2,
        // East-Asian Wide blocks and the emoji planes.
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F9FF
        | 0x1FA70..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// A duration as a human reads it: `840ms`, `12.4s`, `2m 04s`.
///
/// Sub-second work is reported in milliseconds because that is the resolution
/// the difference lives at; past a minute the seconds are zero-padded so a
/// column of them lines up.
pub fn fmt_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1.0 {
        format!("{}ms", d.as_millis())
    } else if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        format!("{}m {:02}s", d.as_secs() / 60, d.as_secs() % 60)
    }
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
///
/// They are also the only writers that bypass [`emit`], so they take the region
/// down first. Every prompt in the CLI happens before anything is pinned today;
/// clearing here makes that hold by construction rather than by call order.
pub fn prompt(label: &str, default: &str) -> String {
    clear_region();
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
    clear_region();
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

    // Colour and emoji are off in these tests by construction: `init` is never
    // called, so both statics stay false. Every renderer is given its `Face`
    // explicitly, so nothing here depends on that or on test order.

    /// Colour on, emoji off — the face that proves colour changes nothing about
    /// the layout.
    const COLOURED: Face = Face {
        color: true,
        emoji: false,
    };
    /// The full treatment.
    const FANCY: Face = Face {
        color: true,
        emoji: true,
    };

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
        assert_eq!(render_row(Face::PLAIN, Level::Ok, "Rust"), "  ok    Rust");
        assert_eq!(render_row(Face::PLAIN, Level::Warn, "Rust"), "  warn  Rust");
        assert_eq!(render_row(Face::PLAIN, Level::Fail, "Rust"), "  fail  Rust");
        assert_eq!(render_row(Face::PLAIN, Level::Info, "Rust"), "        Rust");
    }

    #[test]
    fn coloured_rows_align_with_uncoloured_ones() {
        for level in [Level::Ok, Level::Warn, Level::Fail, Level::Info] {
            assert_eq!(
                strip_ansi(&render_row(COLOURED, level, "Rust")),
                render_row(Face::PLAIN, level, "Rust"),
            );
        }
    }

    #[test]
    fn continuation_lines_up_with_row_text() {
        assert_eq!(CONTINUATION.len(), INDENT.len() + MARKER_WIDTH + GAP);
        let row = render_row(Face::PLAIN, Level::Fail, "x");
        assert_eq!(row.find('x'), Some(CONTINUATION.len()));
    }

    // ── The emoji face ───────────────────────────────────────────────────────

    #[test]
    fn every_icon_is_two_columns_wide() {
        // The load-bearing assumption of the whole emoji face: an icon fits the
        // marker column exactly, so row text lands at CONTINUATION either way.
        // A one-column glyph would shift every row it appears on.
        for glyph in icon::ALL {
            assert_eq!(display_width(glyph), 2, "{glyph} is not two columns");
        }
        for label in ["api", "app", "www", "mail", "prom", "admin", "puzzles"] {
            let glyph = label_icon(label);
            assert_eq!(
                display_width(glyph),
                2,
                "{label}'s {glyph} is not two columns"
            );
        }
    }

    #[test]
    fn emoji_rows_share_the_same_text_column_as_ascii_ones() {
        for level in [Level::Ok, Level::Warn, Level::Fail, Level::Info] {
            let row = render_row(FANCY, level, "Rust");
            assert_eq!(
                display_width(row.split("Rust").next().unwrap()),
                CONTINUATION.len(),
                "{level:?} does not put its text at column 8: {row:?}",
            );
        }
    }

    #[test]
    fn display_width_counts_columns_not_chars() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width(""), 0);
        // Escapes are not on screen.
        assert_eq!(display_width(paint_when(true, GREEN, "ok")), 2);
        // An emoji is two columns; its variation selector and joiner are none.
        assert_eq!(display_width("✅"), 2);
        assert_eq!(display_width("⚠️"), 2);
        assert_eq!(display_width("🦀"), 2);
        assert_eq!(display_width("café"), 4);
    }

    #[test]
    fn fmt_duration_reads_at_the_right_resolution() {
        use std::time::Duration;
        assert_eq!(fmt_duration(Duration::from_millis(0)), "0ms");
        assert_eq!(fmt_duration(Duration::from_millis(840)), "840ms");
        assert_eq!(fmt_duration(Duration::from_millis(1000)), "1.0s");
        assert_eq!(fmt_duration(Duration::from_millis(12_350)), "12.3s");
        assert_eq!(fmt_duration(Duration::from_secs(59)), "59.0s");
        assert_eq!(fmt_duration(Duration::from_secs(124)), "2m 04s");
    }

    #[test]
    fn a_section_leads_with_the_icon_or_the_arrow() {
        assert_eq!(
            render_section(Face::PLAIN, icon::DOCTOR, "Environment"),
            "\n==> Environment",
        );
        assert_eq!(
            render_section(FANCY, icon::DOCTOR, "Environment"),
            format!(
                "\n{} {}",
                icon::DOCTOR,
                paint_when(true, BOLD, "Environment")
            ),
        );
    }

    #[test]
    fn a_result_line_survives_both_faces() {
        assert_eq!(
            render_finished(Face::PLAIN, icon::DONE, "build finished in 1.2s"),
            "done: build finished in 1.2s",
        );
        assert_eq!(
            render_finished(FANCY, icon::DONE, "build finished in 1.2s"),
            format!("{} build finished in 1.2s", icon::DONE),
        );
    }

    #[test]
    fn next_steps_uses_the_existing_two_indents() {
        let steps = ["cd acme".to_string(), "erno dev".to_string()];
        assert_eq!(
            render_next_steps(Face::PLAIN, "Next steps", &steps),
            "  Next steps\n        cd acme\n        erno dev\n",
        );
    }

    #[test]
    fn render_rows_sizes_the_label_column_from_the_widest_label() {
        let rows = vec![
            Row::ok("Rust", "1.90.0"),
            Row::ok("PostgreSQL client", "16.3"),
            Row::fail("psql", "not found\nInstall it"),
        ];
        let text = render_rows(Face::PLAIN, &rows);
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
            render_fatal(Face::PLAIN, "1 required check failed\nFix them and retry."),
            "error: 1 required check failed\n  Fix them and retry.",
        );
        assert_eq!(render_fatal(Face::PLAIN, "boom"), "error: boom");
        assert_eq!(
            render_fatal(
                Face {
                    color: false,
                    emoji: true
                },
                "boom"
            ),
            format!("{} error: boom", icon::FAIL),
        );
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
    fn column_width_measures_columns_not_bytes() {
        assert_eq!(column_width(["a", "bbb"]), 3);
        assert_eq!(column_width([]), 0);
        assert_eq!(column_width(["café"]), 4);
        // An icon costs a column more than its `char` count suggests, which is
        // the whole reason `column_width` cannot go back to counting chars.
        assert_eq!(column_width(["🦀 api", "📊 prom"]), 7);
    }

    #[test]
    fn strip_ansi_removes_escapes_and_keeps_text() {
        assert_eq!(strip_ansi("\u{1b}[31mERROR\u{1b}[0m boom"), "ERROR boom");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    // ── The pinned region ────────────────────────────────────────────────────

    #[test]
    fn a_frame_with_nothing_pinned_is_a_plain_writeln() {
        assert_eq!(render_frame(0, &["hi"], &[]), "hi\n");
        assert_eq!(render_frame(0, &["a", "b"], &[]), "a\nb\n");
        assert_eq!(render_frame(0, &[], &[]), "");
    }

    #[test]
    fn a_frame_erases_what_the_last_one_drew() {
        let region = vec!["one".to_string(), "two".to_string()];
        assert_eq!(
            render_frame(2, &["hi"], &region),
            "\r\u{1b}[2A\u{1b}[0Jhi\none\ntwo\n",
        );
        // The bare erase, used to take the region off the screen.
        assert_eq!(render_frame(2, &[], &[]), "\r\u{1b}[2A\u{1b}[0J");
        // And the bare redraw, used after a stdout write.
        assert_eq!(render_frame(0, &[], &region), "one\ntwo\n");
    }

    #[test]
    fn every_frame_leaves_the_cursor_below_the_region() {
        // The load-bearing invariant: `drawn` next time == the region's height,
        // so the newline count must be exactly body + region.
        let region = ["r1".to_string(), "r2".to_string(), "r3".to_string()];
        for drawn in [0, 1, 3] {
            for body in [vec![], vec!["a"], vec!["a", "b"]] {
                for region in [&[][..], &region[..1], &region[..]] {
                    let text = render_frame(drawn, &body, region);
                    assert_eq!(
                        text.matches('\n').count(),
                        body.len() + region.len(),
                        "drawn={drawn} body={body:?} region={region:?}",
                    );
                }
            }
        }
    }

    #[test]
    fn truncate_display_counts_printable_columns_only() {
        assert_eq!(truncate_display("hello", 10), "hello");
        assert_eq!(truncate_display("hello", 5), "hello");
        assert_eq!(truncate_display("hello", 3), "hel");
        assert_eq!(truncate_display("hello", 0), "");
    }

    #[test]
    fn truncate_display_never_splits_a_wide_glyph() {
        // Half an emoji is not a thing a terminal can draw, so the budget goes
        // unspent rather than overrun.
        assert_eq!(truncate_display("🦀ab", 1), "");
        assert_eq!(truncate_display("🦀ab", 2), "🦀");
        assert_eq!(truncate_display("🦀ab", 3), "🦀a");
        assert_eq!(truncate_display("a⚠️b", 2), "a");
        assert_eq!(display_width(truncate_display("🦀 api", 4)), 4);
    }

    #[test]
    fn truncate_display_never_splits_an_escape() {
        let painted = format!("{}{}", paint_when(true, GREEN, "green"), " tail");
        let cut = truncate_display(&painted, 3);
        assert_eq!(strip_ansi(&cut).chars().count(), 3);
        assert_eq!(strip_ansi(&cut), "gre");
        // A cut mid-style is closed, or the colour bleeds into the next row.
        assert!(cut.ends_with("\u{1b}[0m"));
        // Every escape that survived is whole.
        assert!(!strip_ansi(&cut).contains('\u{1b}'));
    }

    #[test]
    fn fit_region_bounds_both_dimensions() {
        let lines: Vec<String> = (0..12).map(|i| format!("line {i}")).collect();
        let fitted = fit_region(&lines, 80, 5);
        assert_eq!(fitted.len(), 4, "one row is left for the cursor");
        assert_eq!(
            fitted[0], "line 0",
            "the first rows are the ones that matter"
        );

        let long = vec!["x".repeat(60)];
        assert_eq!(fit_region(&long, 20, 40)[0].chars().count(), 19);

        assert!(fit_region(&lines, 2, 40).is_empty());
        assert!(fit_region(&lines, 80, 2).is_empty());
    }

    #[test]
    fn a_terminal_too_small_for_the_whole_region_is_not_pinned_to() {
        let region: Vec<String> = (0..7).map(|i| format!("  row {i} …………………………")).collect();
        let widest = strip_ansi(&region[0]).chars().count();

        assert!(region_fits(&region, 80, 24));
        assert!(!region_fits(&region, 80, 10), "no room to scroll above it");
        assert!(!region_fits(&region, widest, 24), "the last column is cut");
        assert!(region_fits(&region, widest + 1, 24));
        assert!(!region_fits(&[], 80, 24));

        // Colour must not count against the width budget.
        let painted = vec![paint_when(true, GREEN, "abc")];
        assert!(region_fits(&painted, 4, 24));
    }

    #[test]
    fn strip_cursor_control_keeps_colour_and_drops_motion() {
        assert_eq!(
            strip_cursor_control("\u{1b}[31mx\u{1b}[0m"),
            "\u{1b}[31mx\u{1b}[0m"
        );
        assert_eq!(strip_cursor_control("a\u{1b}[2Ab"), "ab");
        assert_eq!(strip_cursor_control("a\rb"), "ab");
        assert_eq!(strip_cursor_control("\u{1b}[0J"), "");
        assert_eq!(strip_cursor_control("plain"), "plain");
    }
}
