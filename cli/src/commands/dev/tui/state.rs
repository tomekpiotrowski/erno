use std::time::Instant;

use super::super::banner::{listed_services, state_named, BannerSnapshot, DevUrls, ServiceState};
use super::super::log::{is_error_line, LogLine};
use super::devapi::{DevJob, MigrationStatus, MockEmail};
use super::loki::LokiLine;
use super::prom::PromSnapshot;
use super::tempo::{Span, TraceHit};

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
    /// How many lines above the newest to keep the cursor. `0` is follow.
    pub log_offset: usize,
    pub paused: bool,
    pub failures_only: bool,
    pub quit: bool,
    pub toast: String,
    pub lens: LensMode,
    pub traces: Vec<TraceHit>,
    pub selected_trace: Option<String>,
    pub spans: Vec<Span>,
    pub loki_lines: Vec<LokiLine>,
    pub prom: PromSnapshot,
    pub emails: Vec<MockEmail>,
    pub jobs: Vec<DevJob>,
    pub migrations: MigrationStatus,
    pub wide_wire: bool,
    pub tempo_query: String,
    /// Next draw should wipe the physical screen so a shorter log cannot leave
    /// cells from the previous service. Set on focus changes.
    pub force_redraw: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LensMode {
    Service,
    Trace,
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
            paused: false,
            failures_only: false,
            quit: false,
            toast: String::new(),
            lens: LensMode::Service,
            traces: Vec::new(),
            selected_trace: None,
            spans: Vec::new(),
            loki_lines: Vec::new(),
            prom: PromSnapshot::default(),
            emails: Vec::new(),
            jobs: Vec::new(),
            migrations: MigrationStatus::default(),
            wide_wire: false,
            tempo_query: "{}".into(),
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
        if !self.paused {
            self.logs = lines;
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

    pub fn visible_traces(&self) -> Vec<&TraceHit> {
        self.traces
            .iter()
            .filter(|hit| match self.focus.and_then(|i| self.services.get(i)) {
                Some(svc) => trace_belongs_to(&hit.service, &svc.name),
                None => true,
            })
            .collect()
    }

    pub fn elapsed(&self) -> String {
        fmt_elapsed(self.boot.elapsed().as_secs())
    }
}

/// Tempo labels the API by its OTEL resource (`erno` by default), not the TUI row `api`.
pub fn trace_belongs_to(hit_service: &str, svc_name: &str) -> bool {
    if hit_service.eq_ignore_ascii_case(svc_name) {
        return true;
    }
    svc_name == "api" && hit_service.eq_ignore_ascii_case("erno")
}

pub fn service_meta(name: &str) -> (&'static str, &'static str) {
    match name {
        "api" => ("cargo run", "api/"),
        "app" => ("npm start", "app/"),
        "www" => ("npm run dev", "www/"),
        "prom" => ("prometheus", ".erno/prometheus"),
        "tempo" => ("tempo", ".erno/tempo"),
        "loki" => ("loki", ".erno/loki"),
        "admin" => ("npm start", "admin/"),
        "console" => ("npm start", "monitoring/ui"),
        "mon" => ("cargo run", "monitoring/"),
        _ => ("", ""),
    }
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
        assert!(names.contains(&"prom"));
        assert!(names.contains(&"tempo"));
        assert!(names.contains(&"loki"));
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

    fn hit(service: &str, name: &str) -> TraceHit {
        TraceHit {
            trace_id: name.into(),
            name: name.into(),
            service: service.into(),
            duration_ms: 1.0,
            start_unix_nano: String::new(),
        }
    }

    #[test]
    fn traces_for_api_include_the_erno_otel_name() {
        assert!(trace_belongs_to("erno", "api"));
        assert!(trace_belongs_to("api", "api"));
        assert!(!trace_belongs_to("erno", "app"));
        assert!(trace_belongs_to("app", "app"));
    }

    #[test]
    fn focused_service_hides_other_traces_and_logs() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        let app = state
            .services
            .iter()
            .position(|s| s.name == "app")
            .expect("app");
        state.focus = Some(app);
        state.logs = vec![
            LogLine {
                label: "api".into(),
                line: "api-only".into(),
            },
            LogLine {
                label: "app".into(),
                line: "app-only".into(),
            },
        ];
        state.traces = vec![hit("erno", "GET /x"), hit("app", "HMR")];
        let logs: Vec<&str> = state
            .visible_logs()
            .iter()
            .map(|l| l.line.as_str())
            .collect();
        let traces: Vec<&str> = state
            .visible_traces()
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(logs, ["app-only"]);
        assert_eq!(traces, ["HMR"]);
    }
}
