mod devapi;
mod draw;
mod editor;
mod keys;
mod loki;
mod prom;
mod state;
mod tempo;

use std::collections::HashMap;
use std::io::{self, IsTerminal};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use super::banner::{self, DevUrls};
use super::log::LogSink;
use super::open;
use super::process::Supervisor;
use crate::ui;
use keys::Action;
use state::LensMode;

pub use state::TuiState;

pub const MIN_COLS: usize = 80;
pub const MIN_ROWS: usize = 24;

/// Inputs that decide whether `erno dev` takes over the screen.
///
/// Tested as a pure struct so `cargo test` does not depend on a TTY.
#[derive(Clone, Debug)]
pub struct TuiGate {
    pub unix: bool,
    pub stderr_tty: bool,
    pub color: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub no_ui: bool,
    pub sticky_off: bool,
    pub cols: usize,
    pub rows: usize,
}

impl TuiGate {
    pub fn from_env(no_ui: bool) -> Self {
        let (cols, rows) = crossterm::terminal::size()
            .map(|(c, r)| (c as usize, r as usize))
            .unwrap_or((0, 0));
        Self {
            unix: cfg!(unix),
            stderr_tty: io::stderr().is_terminal(),
            color: ui::color(),
            quiet: ui::quiet(),
            verbose: ui::verbose(),
            no_ui,
            sticky_off: std::env::var("ERNO_STICKY").as_deref() == Ok("0"),
            cols,
            rows,
        }
    }

    pub fn should_start(&self) -> bool {
        self.unix
            && self.stderr_tty
            && self.color
            && !self.quiet
            && !self.verbose
            && !self.no_ui
            && !self.sticky_off
            && self.cols >= MIN_COLS
            && self.rows >= MIN_ROWS
    }
}

struct TuiGuard;

impl TuiGuard {
    fn enter() -> Result<Self, String> {
        enable_raw_mode().map_err(|e| format!("could not enable raw mode: {e}"))?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(format!("could not enter the alternate screen: {e}"));
        }
        ui::set_tui_live(true);
        ui::set_fatal_hook(Some(restore_terminal));
        Ok(Self)
    }
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        ui::set_fatal_hook(None);
        restore_terminal();
    }
}

fn restore_terminal() {
    ui::set_tui_live(false);
    let _ = disable_raw_mode();
    let mut out = io::stdout();
    let _ = execute!(out, Show, LeaveAlternateScreen);
}

#[derive(Clone)]
pub struct TuiOpts {
    pub prometheus: Option<String>,
    pub prometheus_token: Option<String>,
    pub tempo: Option<String>,
    pub loki: Option<String>,
    pub api: Option<String>,
    pub console: Option<String>,
}

pub async fn run(
    urls: &DevUrls,
    sink: Arc<LogSink>,
    project: &str,
    supervisors: HashMap<String, Supervisor>,
    opts: TuiOpts,
) -> Result<(), String> {
    let probe_client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let action_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut state = TuiState::new(project, urls);
    let (packet_tx, mut packet_rx) = tokio::sync::mpsc::channel(1);
    let (want_tx, want_rx) = tokio::sync::watch::channel(FetchWant::from_state(&state));
    tokio::spawn(fetch_loop(
        FetchCtl {
            urls: urls.clone(),
            opts: opts.clone(),
            client: probe_client,
        },
        packet_tx,
        want_rx,
    ));

    let _guard = TuiGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal =
        Terminal::new(backend).map_err(|e| format!("could not start the dashboard: {e}"))?;
    loop {
        while event::poll(Duration::ZERO).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let action = keys::interpret(key);
                keys::apply(&mut state, action);
                handle_action(&mut state, action, &supervisors, &opts, &action_client).await;
            }
        }
        if state.quit {
            break;
        }
        let _ = want_tx.send_if_modified(|cur| {
            let next = FetchWant::from_state(&state);
            if *cur == next {
                false
            } else {
                *cur = next;
                true
            }
        });
        while let Ok(packet) = packet_rx.try_recv() {
            apply_packet(&mut state, urls, packet);
        }
        tick_local(&mut state, &sink, &supervisors).await;
        if state.force_redraw {
            terminal
                .clear()
                .map_err(|e| format!("could not clear the dashboard: {e}"))?;
            state.force_redraw = false;
        }
        terminal
            .draw(|f| draw::render(f, &state))
            .map_err(|e| format!("could not draw the dashboard: {e}"))?;
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FetchWant {
    tempo_query: String,
    selected_trace: Option<String>,
}

impl FetchWant {
    fn from_state(state: &TuiState) -> Self {
        Self {
            tempo_query: state.tempo_query.clone(),
            selected_trace: state.selected_trace.clone(),
        }
    }
}

struct FetchCtl {
    urls: DevUrls,
    opts: TuiOpts,
    client: reqwest::Client,
}

struct FetchPacket {
    snap: banner::BannerSnapshot,
    prom: Option<prom::PromSnapshot>,
    traces: Option<Vec<tempo::TraceHit>>,
    spans: Option<Vec<tempo::Span>>,
    loki: Option<Vec<loki::LokiLine>>,
    emails: Option<Vec<devapi::MockEmail>>,
    jobs: Option<Vec<devapi::DevJob>>,
    migrations: Option<devapi::MigrationStatus>,
}

struct FetchClocks {
    prom: Instant,
    tempo: Instant,
    dev: Instant,
}

fn apply_packet(state: &mut TuiState, urls: &DevUrls, packet: FetchPacket) {
    state.apply_snapshot(urls, &packet.snap);
    if let Some(prom) = packet.prom {
        state.prom = prom;
    }
    if let Some(traces) = packet.traces {
        state.traces = traces;
    }
    if let Some(spans) = packet.spans {
        state.spans = spans;
    }
    if let Some(loki) = packet.loki {
        state.loki_lines = loki;
    }
    if let Some(emails) = packet.emails {
        state.emails = emails;
    }
    if let Some(jobs) = packet.jobs {
        state.jobs = jobs;
    }
    if let Some(migrations) = packet.migrations {
        state.migrations = migrations;
    }
}

async fn tick_local(
    state: &mut TuiState,
    sink: &Arc<LogSink>,
    supervisors: &HashMap<String, Supervisor>,
) {
    state.ingest_logs(sink.recent());
    for svc in &mut state.services {
        if let Some(sup) = supervisors.get(&svc.name) {
            svc.pid = sup.pid().await;
        }
    }
}

async fn fetch_loop(
    ctl: FetchCtl,
    tx: tokio::sync::mpsc::Sender<FetchPacket>,
    mut want_rx: tokio::sync::watch::Receiver<FetchWant>,
) {
    let aged = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);
    let mut clocks = FetchClocks {
        prom: aged,
        tempo: aged,
        dev: aged,
    };
    let mut prev_prom = prom::PromSnapshot::default();
    loop {
        let want = want_rx.borrow().clone();
        let snap = banner::probe_all(&ctl.client, &ctl.urls).await;
        let mut packet = FetchPacket {
            snap,
            prom: None,
            traces: None,
            spans: None,
            loki: None,
            emails: None,
            jobs: None,
            migrations: None,
        };
        if clocks.prom.elapsed() > Duration::from_secs(2) {
            if let Some(base) = &ctl.opts.prometheus {
                prev_prom = prom::fetch(
                    &ctl.client,
                    base,
                    ctl.opts.prometheus_token.as_deref(),
                    &prev_prom,
                )
                .await;
                packet.prom = Some(prev_prom.clone());
            }
            clocks.prom = Instant::now();
        }
        if clocks.tempo.elapsed() > Duration::from_secs(1) {
            if let Some(base) = &ctl.opts.tempo {
                let selected = want.selected_trace.clone();
                let loki_base = ctl.opts.loki.clone();
                let (traces, spans, loki_lines) = tokio::join!(
                    tempo::search(&ctl.client, base, &want.tempo_query, 60, 40),
                    async {
                        match &selected {
                            Some(id) => tempo::get_trace(&ctl.client, base, id).await,
                            None => Vec::new(),
                        }
                    },
                    async {
                        match (&selected, &loki_base) {
                            (Some(id), Some(loki_url)) => {
                                loki::range(&ctl.client, loki_url, &loki::build_logql(id), 3600)
                                    .await
                            }
                            _ => Vec::new(),
                        }
                    },
                );
                packet.traces = Some(traces);
                packet.spans = Some(spans);
                packet.loki = Some(loki_lines);
            }
            clocks.tempo = Instant::now();
        }
        if clocks.dev.elapsed() > Duration::from_secs(2) {
            if let Some(api) = &ctl.opts.api {
                let (emails, jobs, migrations) = tokio::join!(
                    devapi::emails(&ctl.client, api),
                    devapi::jobs(&ctl.client, api),
                    devapi::migrations(&ctl.client, api),
                );
                packet.emails = Some(emails);
                packet.jobs = Some(jobs);
                packet.migrations = Some(migrations);
            }
            clocks.dev = Instant::now();
        }
        match tx.try_send(packet) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            changed = want_rx.changed() => {
                if changed.is_err() {
                    break;
                }
            }
        }
    }
}

async fn handle_action(
    state: &mut TuiState,
    action: Action,
    supervisors: &HashMap<String, Supervisor>,
    opts: &TuiOpts,
    client: &reqwest::Client,
) {
    match action {
        Action::Restart => {
            if let Some(name) = state.target_service().map(|s| s.name.clone()) {
                if let Some(sup) = supervisors.get(&name).cloned() {
                    tokio::spawn(async move {
                        sup.restart().await;
                    });
                }
            }
        }
        Action::Open => {
            if state.lens == LensMode::Trace {
                if let (Some(id), Some(console)) = (state.selected_trace.clone(), &opts.console) {
                    let url = format!("{}/performance/traces/{id}", console.trim_end_matches('/'));
                    let _ = open::open_browser(&url);
                    state.say(format!("open {url}"));
                    return;
                }
            }
            if state.lens == LensMode::Mail {
                if let (Some(m), Some(api)) = (state.emails.first(), &opts.api) {
                    let url = format!("{}/dev/emails/{}/preview", api.trim_end_matches('/'), m.id);
                    let _ = open::open_browser(&url);
                    return;
                }
            }
            if let Some(url) = state.target_service().map(|s| s.url.clone()) {
                let _ = open::open_browser(&url);
            }
        }
        Action::Enter => {
            if state.lens == LensMode::Jobs {
                if let (Some(api), Some(j)) = (
                    opts.api.as_deref(),
                    state.jobs.iter().find(|j| j.status == "failed"),
                ) {
                    let id = j.id.clone();
                    devapi::retry_job(client, api, &id).await;
                    state.say(format!("retry {id}"));
                }
                return;
            }
            let traces = state.visible_traces();
            if let Some(id) = traces.first().map(|t| t.trace_id.clone()) {
                let idx = state.log_offset.min(traces.len().saturating_sub(1));
                let id = traces.get(idx).map(|t| t.trace_id.clone()).unwrap_or(id);
                state.selected_trace = Some(id);
                state.lens = LensMode::Trace;
            }
        }
        Action::Slowest => {
            if let Some(hit) = state
                .traces
                .iter()
                .max_by(|a, b| a.duration_ms.total_cmp(&b.duration_ms))
            {
                state.selected_trace = Some(hit.trace_id.clone());
                state.lens = LensMode::Trace;
                state.say(format!("slowest · {:.0}ms", hit.duration_ms));
            }
        }
        Action::Editor => {
            if let Some((file, line)) = editor_target(state) {
                match editor::open(&file, line) {
                    Ok(()) => state.say(format!("{file}:{line} → editor")),
                    Err(e) => state.say(e),
                }
            } else {
                state.say("no file:line on this trace");
            }
        }
        Action::Copy => {
            if state.lens == LensMode::Mail {
                if let (Some(api), Some(m)) = (opts.api.as_deref(), state.emails.first()) {
                    let id = m.id.clone();
                    devapi::delete_email(client, api, &id).await;
                    state.say("dismissed mail");
                    return;
                }
            }
            copy_visible_log(state);
        }
        Action::Migrate => {
            if let Some(api) = &opts.api {
                match devapi::migrate_up(client, api).await {
                    Ok(name) => state.say(format!("applied {name}")),
                    Err(e) => state.say(e),
                }
            }
        }
        Action::Revert => {
            if let Some(api) = &opts.api {
                match devapi::migrate_down(client, api).await {
                    Ok(name) => state.say(format!("reverted {name}")),
                    Err(e) => state.say(e),
                }
            }
        }
        _ => {}
    }
}

fn editor_target(state: &TuiState) -> Option<(String, u32)> {
    loki::panic_frames(&state.loki_lines).into_iter().next()
}

fn copy_visible_log(state: &mut TuiState) {
    let n = state.visible_logs().len();
    if n == 0 {
        state.say("nothing to copy");
        return;
    }
    match copy_text(&state.visible_log_text()) {
        Ok(()) => state.say(copied_toast(n)),
        Err(()) => state.say("no clipboard (wl-copy/xclip)"),
    }
}

fn copied_toast(n: usize) -> String {
    if n == 1 {
        "copied 1 line".into()
    } else {
        format!("copied {n} lines")
    }
}

fn copy_text(text: &str) -> Result<(), ()> {
    const TOOLS: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("pbcopy", &[]),
    ];
    for (bin, args) in TOOLS {
        let mut cmd = std::process::Command::new(bin);
        cmd.args(*args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let Ok(mut child) = cmd.spawn() else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return Ok(());
        }
    }
    Err(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> TuiGate {
        TuiGate {
            unix: true,
            stderr_tty: true,
            color: true,
            quiet: false,
            verbose: false,
            no_ui: false,
            sticky_off: false,
            cols: 120,
            rows: 40,
        }
    }

    #[test]
    fn tty_colour_terminal_starts_the_dashboard() {
        assert!(gate().should_start());
    }

    #[test]
    fn no_ui_keeps_the_banner() {
        let mut g = gate();
        g.no_ui = true;
        assert!(!g.should_start());
    }

    #[test]
    fn quiet_verbose_pipe_and_small_terminals_fall_back() {
        for tweak in [
            TuiGate {
                quiet: true,
                ..gate()
            },
            TuiGate {
                verbose: true,
                ..gate()
            },
            TuiGate {
                stderr_tty: false,
                ..gate()
            },
            TuiGate {
                color: false,
                ..gate()
            },
            TuiGate {
                sticky_off: true,
                ..gate()
            },
            TuiGate {
                unix: false,
                ..gate()
            },
            TuiGate { cols: 79, ..gate() },
            TuiGate { rows: 23, ..gate() },
        ] {
            assert!(!tweak.should_start(), "{tweak:?}");
        }
    }

    #[test]
    fn copied_toast_counts_lines() {
        assert_eq!(copied_toast(1), "copied 1 line");
        assert_eq!(copied_toast(12), "copied 12 lines");
    }
}
