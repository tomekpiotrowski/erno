use std::collections::HashMap;
use std::time::Instant;

use super::super::banner::{listed_services, state_named, BannerSnapshot, DevUrls, ServiceState};
use super::super::log::{classify_wire_line, is_error_line, LogLine, WireKind};
use super::devapi::{DevJob, MigrationStatus, MockEmail};

#[derive(Clone, Debug)]
pub struct ServiceRow {
    pub num: usize,
    pub name: String,
    pub url: String,
    pub state: ServiceState,
    pub cmd: String,
    pub watch: String,
    pub pid: Option<u32>,
}

#[derive(Debug)]
pub struct TuiState {
    pub project: String,
    pub boot: Instant,
    pub services: Vec<ServiceRow>,
    pub logs: Vec<LogLine>,
    /// `None` means every service (`0`). Otherwise an index into `services`.
    pub focus: Option<usize>,
    /// Newest log rows sitting below the pane. `0` is follow (pinned to the end).
    pub log_offset: usize,
    /// Inner height of the LOG pane, set from the terminal size each tick.
    /// Caps `log_offset` so ↑ cannot hide lines that still fit.
    pub log_view_height: usize,
    pub paused: bool,
    pub failures_only: bool,
    pub quit: bool,
    pub toast: String,
    pub lens: LensMode,
    pub emails: Vec<MockEmail>,
    pub jobs: Vec<DevJob>,
    pub migrations: MigrationStatus,
    pub wide_wire: bool,
    /// Request / reload / error marks keyed by service, taken from child logs.
    pub wire_ticks: HashMap<String, Vec<WireTick>>,
    /// Last log `seq` ingested into `wire_ticks`. `None` until the first
    /// sample, which is a baseline so historical lines are not a burst.
    log_cursor: Option<u64>,
    /// Next draw should wipe the physical screen so a shorter log cannot leave
    /// cells from the previous service. Set on focus changes.
    pub force_redraw: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct WireTick {
    pub at: f64,
    pub error: bool,
    pub reload: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LensMode {
    Service,
    Mail,
    Jobs,
}

impl TuiState {
    pub fn new(project: impl Into<String>, urls: &DevUrls) -> Self {
        let services = listed_services(urls)
            .into_iter()
            .enumerate()
            .map(|(i, (name, url))| {
                let (cmd, watch) = service_meta(&name);
                ServiceRow {
                    num: i + 1,
                    name,
                    url,
                    state: ServiceState::Starting,
                    cmd: cmd.into(),
                    watch: watch.into(),
                    pid: None,
                }
            })
            .collect();
        Self {
            project: project.into(),
            boot: Instant::now(),
            services,
            logs: Vec::new(),
            focus: None,
            log_offset: 0,
            log_view_height: 0,
            paused: false,
            failures_only: false,
            quit: false,
            toast: String::new(),
            lens: LensMode::Service,
            emails: Vec::new(),
            jobs: Vec::new(),
            migrations: MigrationStatus::default(),
            wide_wire: false,
            wire_ticks: HashMap::new(),
            log_cursor: None,
            force_redraw: false,
        }
    }

    pub fn apply_snapshot(&mut self, urls: &DevUrls, snap: &BannerSnapshot) {
        for svc in &mut self.services {
            if let Some(st) = state_named(snap, urls, &svc.name) {
                svc.state = st;
            }
        }
    }

    pub fn target_service(&self) -> Option<&ServiceRow> {
        self.focus
            .and_then(|i| self.services.get(i))
            .or(self.services.first())
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.toast = msg.into();
    }

    pub fn ingest_logs(&mut self, lines: Vec<LogLine>) {
        self.ingest_wire_ticks(&lines);
        if !self.paused {
            self.logs = lines;
        }
    }

    pub fn ticks(&self, service: &str) -> &[WireTick] {
        self.wire_ticks
            .get(service)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn push_tick(&mut self, service: &str, now: f64, error: bool, reload: bool) {
        self.wire_ticks
            .entry(service.to_string())
            .or_default()
            .push(WireTick {
                at: now,
                error,
                reload,
            });
    }

    fn prune_ticks(&mut self, now: f64) {
        for ticks in self.wire_ticks.values_mut() {
            ticks.retain(|t| now - t.at <= 300.0);
        }
    }

    /// Every lane is the same: new child log lines that classify as HTTP,
    /// HMR, or error. erno's own probes never reach the ring.
    fn ingest_wire_ticks(&mut self, lines: &[LogLine]) {
        let Some(max) = lines.iter().map(|l| l.seq).max() else {
            return;
        };
        let Some(cursor) = self.log_cursor else {
            self.log_cursor = Some(max);
            return;
        };
        let now = unix_now();
        for line in lines {
            if line.seq <= cursor {
                continue;
            }
            match classify_wire_line(&line.line) {
                Some(WireKind::Request { error }) => {
                    self.push_tick(&line.label, now, error, false);
                }
                Some(WireKind::Reload) => {
                    self.push_tick(&line.label, now, false, true);
                }
                None => {}
            }
        }
        self.log_cursor = Some(max.max(cursor));
        self.prune_ticks(now);
    }

    pub fn log_row_count(&self) -> usize {
        self.visible_logs().len()
    }

    pub fn max_log_offset(&self) -> usize {
        self.log_row_count().saturating_sub(self.log_view_height)
    }

    pub fn clamp_log_offset(&mut self) {
        let max = self.max_log_offset();
        if self.log_offset > max {
            self.log_offset = max;
        }
    }

    pub fn visible_logs(&self) -> Vec<&LogLine> {
        self.logs
            .iter()
            .filter(|line| {
                if let Some(i) = self.focus {
                    if self.services.get(i).is_none_or(|s| s.name != line.label) {
                        return false;
                    }
                }
                if self.failures_only && !is_error_line(&line.line) {
                    return false;
                }
                true
            })
            .collect()
    }

    /// Clipboard text for the LOG pane: the current service / failures filter,
    /// untruncated and with ANSI stripped, matching `.erno/dev.log`.
    pub fn visible_log_text(&self) -> String {
        let mut out = String::new();
        for line in self.visible_logs() {
            let body = crate::ui::strip_ansi(&line.line);
            out.push('[');
            out.push_str(&line.label);
            out.push_str("] ");
            out.push_str(&body);
            out.push('\n');
        }
        out
    }

    pub fn elapsed(&self) -> String {
        fmt_elapsed(self.boot.elapsed().as_secs())
    }
}

pub fn service_meta(name: &str) -> (&'static str, &'static str) {
    match name {
        "api" => ("cargo run", "api/"),
        "app" => ("bun run start", "app/"),
        "www" => ("bun run dev", "www/"),
        "admin" => ("bun run start", "admin/"),
        _ => ("", ""),
    }
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Visible `[start, end)` of a follow-the-bottom log.
///
/// `offset` is how many newest rows sit below the window (`0` = follow).
/// The window never shrinks below `min(row_count, view_h)`: when the lines
/// fit, offset is ignored; when they overflow, the pane stays full.
pub fn log_window(row_count: usize, view_h: usize, offset: usize) -> (usize, usize) {
    let offset = offset.min(row_count.saturating_sub(view_h));
    let end = row_count.saturating_sub(offset);
    let start = end.saturating_sub(view_h);
    (start, end)
}

pub fn fmt_elapsed(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_lists_started_services_in_banner_order() {
        let urls = DevUrls::defaults(true, true, true);
        let state = TuiState::new("teryon", &urls);
        let names: Vec<&str> = state.services.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"api"));
        assert!(names.contains(&"app"));
        assert!(names.contains(&"www"));
        assert_eq!(state.services[0].num, 1);
        assert!(state.focus.is_none());
        assert_eq!(state.target_service().unwrap().name, names[0]);
        assert_eq!(service_meta("api"), ("cargo run", "api/"));
    }

    #[test]
    fn elapsed_formats_hours_then_minutes() {
        assert_eq!(fmt_elapsed(4), "4s");
        assert_eq!(fmt_elapsed(65), "1m05s");
        assert_eq!(fmt_elapsed(3852), "1h04m");
    }
    #[test]
    fn visible_log_text_matches_the_file_log_and_strips_ansi() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        let app = state
            .services
            .iter()
            .position(|s| s.name == "app")
            .expect("app");
        state.focus = Some(app);
        state.logs = vec![
            LogLine::new("api", "api-only"),
            LogLine::new("app", "\u{1b}[31merror boom\u{1b}[0m"),
            LogLine::new("app", "still going"),
        ];
        assert_eq!(
            state.visible_log_text(),
            "[app] error boom\n[app] still going\n"
        );
        state.failures_only = true;
        assert_eq!(state.visible_log_text(), "[app] error boom\n");
        state.focus = None;
        state.logs.clear();
        assert!(state.visible_log_text().is_empty());
    }

    #[test]
    fn log_window_keeps_every_line_when_they_fit() {
        assert_eq!(log_window(5, 20, 0), (0, 5));
        assert_eq!(log_window(5, 20, 1), (0, 5));
        assert_eq!(log_window(5, 20, 99), (0, 5));
        assert_eq!(log_window(0, 20, 3), (0, 0));
    }

    #[test]
    fn log_window_scrolls_the_newest_out_only_when_the_pane_is_full() {
        assert_eq!(log_window(10, 4, 0), (6, 10));
        assert_eq!(log_window(10, 4, 1), (5, 9));
        assert_eq!(log_window(10, 4, 6), (0, 4));
        assert_eq!(log_window(10, 4, 99), (0, 4));
    }

    fn log(label: &str, line: &str) -> LogLine {
        LogLine::new(label, line)
    }

    #[test]
    fn max_log_offset_is_the_overflow_not_the_line_count() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        state.log_view_height = 8;
        state.logs = (0..5).map(|i| log("api", &format!("l{i}"))).collect();
        assert_eq!(state.max_log_offset(), 0);
        state.logs = (0..20).map(|i| log("api", &format!("l{i}"))).collect();
        assert_eq!(state.max_log_offset(), 12);
        state.log_offset = 99;
        state.clamp_log_offset();
        assert_eq!(state.log_offset, 12);
    }

    fn seq(n: u64, label: &str, line: &str) -> LogLine {
        LogLine {
            seq: n,
            ..LogLine::new(label, line)
        }
    }

    #[test]
    fn wire_ticks_come_from_child_logs_on_every_service() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        state.ingest_logs(vec![seq(1, "www", "22:41:58 [200] / 2ms")]);
        assert!(
            state.ticks("www").is_empty(),
            "first sample is a baseline, not a burst"
        );
        state.ingest_logs(vec![
            seq(1, "www", "22:41:58 [200] / 2ms"),
            seq(2, "www", "22:41:59 [200] / 3ms"),
            seq(3, "www", "22:42:01 [200] HEAD / 2ms"),
            seq(4, "www", "22:42:02 [500] /broken 12ms"),
            seq(
                5,
                "api",
                "23:17:00.03 DEBUG [200] GET /api/projects 4ms",
            ),
            seq(
                6,
                "api",
                "request: finished processing request latency=1 ms status=200 method=GET uri=/readiness version=HTTP/1.1",
            ),
            seq(7, "app", "✔ Changes detected. Rebuilding..."),
            seq(8, "www", "compiled successfully"),
            seq(9, "api", "Worker 'default-0' received job notification"),
        ]);
        assert_eq!(state.ticks("www").len(), 2);
        assert!(state.ticks("www")[1].error);
        assert_eq!(state.ticks("api").len(), 1);
        assert!(!state.ticks("api")[0].error);
        assert_eq!(state.ticks("app").len(), 1);
        assert!(state.ticks("app")[0].reload);
    }
}
