use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::prom::spark;
use super::state::{trace_belongs_to, LensMode, ServiceRow, TuiState};
use super::tempo::{flatten, n1_insight};

const ACCENT: Color = Color::Rgb(145, 132, 217);
const DIM: Color = Color::Rgb(117, 121, 140);
const OK: Color = Color::Rgb(94, 184, 158);
const WARN: Color = Color::Rgb(201, 168, 92);
const ERR: Color = Color::Rgb(196, 92, 78);

pub fn render(frame: &mut Frame, state: &TuiState) {
    frame.render_widget(Clear, frame.area());
    let area = frame.area();
    let show_wire = area.width >= 152 && area.height >= 24;
    let mut vert = vec![Constraint::Length(3), Constraint::Min(8)];
    if show_wire {
        vert.push(Constraint::Length(8));
    }
    vert.push(Constraint::Length(2));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vert)
        .split(area);

    render_header(frame, chunks[0], state);

    let wide = area.width >= 152;
    let mid = area.width >= 100;
    let body = chunks[1];
    if wide {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(28),
                Constraint::Min(40),
                Constraint::Length(44),
            ])
            .split(body);
        render_services(frame, cols[0], state);
        render_log(frame, cols[1], state);
        render_lens(frame, cols[2], state);
    } else if mid {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(40)])
            .split(body);
        render_services(frame, cols[0], state);
        render_log(frame, cols[1], state);
    } else {
        render_log(frame, body, state);
    }

    let mut idx = 2;
    if show_wire {
        render_wire(frame, chunks[idx], state);
        idx += 1;
    }
    render_footer(frame, chunks[idx], state, wide);
}

fn render_header(frame: &mut Frame, area: Rect, state: &TuiState) {
    let ready = state
        .services
        .iter()
        .filter(|s| s.state.label() == "ready")
        .count();
    let status = format!("{ready}/{} up", state.services.len());
    let p95 = format!("p95 {:.0}ms", state.prom.p95_ms);
    let err = format!("err {:.2}/s", state.prom.err_per_s);
    let title = Line::from(vec![
        Span::styled("◈ erno dev", Style::default().fg(ACCENT)),
        Span::raw("  "),
        Span::styled(&state.project, Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(status, Style::default().fg(OK)),
        Span::raw("  "),
        Span::styled(state.elapsed(), Style::default().fg(DIM)),
        Span::raw("  "),
        Span::styled(p95, Style::default().fg(DIM)),
        Span::raw("  "),
        Span::styled(err, Style::default().fg(ERR)),
        Span::raw("  "),
        Span::styled(spark(&state.prom.req_hist), Style::default().fg(ACCENT)),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::BOTTOM)),
        area,
    );
}

fn render_services(frame: &mut Frame, area: Rect, state: &TuiState) {
    let mut lines = vec![Line::from(Span::styled(
        "SERVICES",
        Style::default().fg(DIM).add_modifier(Modifier::BOLD),
    ))];
    for (i, svc) in state.services.iter().enumerate() {
        let focused = state.focus == Some(i);
        let style = if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default()
        };
        let (dot, color) = match svc.state.label() {
            "ready" => ("●", OK),
            "migrating" => ("◐", WARN),
            _ => ("◌", WARN),
        };
        let port = svc
            .url
            .rsplit(':')
            .next()
            .map(|p| format!(":{p}"))
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(format!("{dot} "), Style::default().fg(color)),
            Span::styled(format!("{} {}", svc.num, svc.name), style),
            Span::styled(format!(" {port}"), Style::default().fg(DIM)),
        ]));
        lines.push(Line::from(Span::styled(
            format!("  {}", svc.state.label()),
            Style::default().fg(DIM),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "BACKING",
        Style::default().fg(DIM).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        format!("postgres  ●  pool {:.0}", state.prom.pool),
        Style::default().fg(DIM),
    )));
    if !state.emails.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("mail      {} held", state.emails.len()),
            Style::default().fg(DIM),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "MIGRATIONS",
        Style::default().fg(DIM).add_modifier(Modifier::BOLD),
    )));
    let head = state.migrations.head.as_deref().unwrap_or("—");
    lines.push(Line::from(Span::styled(
        format!("head {head}"),
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "{} applied · {} pending",
            state.migrations.applied.len(),
            state.migrations.pending.len()
        ),
        Style::default().fg(if state.migrations.pending.is_empty() {
            DIM
        } else {
            WARN
        }),
    )));
    lines.push(Line::from(Span::styled(
        "m migrate · M revert",
        Style::default().fg(DIM),
    )));
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::RIGHT)),
        area,
    );
}

fn render_log(frame: &mut Frame, area: Rect, state: &TuiState) {
    let scope = match state.focus.and_then(|i| state.services.get(i)) {
        Some(s) => s.name.as_str(),
        None => "all",
    };
    let hint = if state.failures_only { "failures" } else { "" };
    let follow = if state.paused { "paused" } else { "follow ▸" };
    let title = format!("LOG  {scope}  {hint}   {follow}");
    let inner_w = area.width.saturating_sub(2) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;
    let mut rows: Vec<Line> = Vec::new();
    for hit in state.visible_traces() {
        let ms = format!("{:.0}ms", hit.duration_ms);
        rows.push(log_row(&hit.service, &hit.name, Some(&ms), inner_w));
    }
    for l in state.visible_logs() {
        rows.push(log_row(&l.label, &l.line, None, inner_w));
    }
    let end = rows.len().saturating_sub(state.log_offset);
    let start = end.saturating_sub(inner_h);
    let mut shown = if start < end {
        rows[start..end].to_vec()
    } else {
        Vec::new()
    };
    while shown.len() < inner_h {
        shown.push(blank_row(inner_w));
    }
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(shown).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(DIM)),
        ),
        area,
    );
}

fn log_row(label: &str, text: &str, extra: Option<&str>, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::from("");
    }
    let label = pad_label(label);
    let extra = extra
        .map(plain_text)
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    let extra_w = if extra.is_empty() {
        0
    } else {
        1 + crate::ui::display_width(&extra)
    };
    let text_budget = width.saturating_sub(7 + 1 + extra_w);
    let text = crate::ui::truncate_display(&plain_text(text), text_budget);
    let used = 7 + 1 + crate::ui::display_width(&text) + extra_w;
    let pad = width.saturating_sub(used);
    let mut spans = vec![
        Span::styled(label, Style::default().fg(ACCENT)),
        Span::raw(" "),
        Span::styled(text, Style::default().fg(Color::Gray)),
    ];
    if !extra.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(extra, Style::default().fg(DIM)));
    }
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    Line::from(spans)
}

fn blank_row(width: usize) -> Line<'static> {
    Line::from(Span::raw(" ".repeat(width)))
}

fn pad_label(label: &str) -> String {
    let mut s = crate::ui::truncate_display(&plain_text(label), 7);
    while crate::ui::display_width(&s) < 7 {
        s.push(' ');
    }
    s
}

fn plain_text(s: &str) -> String {
    crate::ui::strip_ansi(s).replace(['\r', '\n'], "")
}

fn render_lens(frame: &mut Frame, area: Rect, state: &TuiState) {
    let (title, body) = match state.lens {
        LensMode::Service => service_lens(state),
        LensMode::Trace => trace_lens(state),
        LensMode::Mail => mail_lens(state),
        LensMode::Jobs => jobs_lens(state),
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(DIM)),
        ),
        area,
    );
}

fn service_lens(state: &TuiState) -> (String, Vec<Line<'static>>) {
    let Some(svc) = state.target_service() else {
        return (
            "LENS  service".into(),
            vec![Line::from(Span::styled(
                "select a service",
                Style::default().fg(DIM),
            ))],
        );
    };
    let pid = svc.pid.map(|p| p.to_string()).unwrap_or_else(|| "—".into());
    let mut lines = vec![
        Line::from(Span::styled(
            svc.name.clone(),
            Style::default().fg(Color::White),
        )),
        fact_owned("cmd", svc.cmd.clone()),
        fact_owned("pid", pid),
        fact_owned("watch", svc.watch.clone()),
        fact_owned("state", svc.state.label().to_string()),
        fact_owned("url", svc.url.clone()),
        Line::from(Span::styled(
            format!(
                "p50 {}  p95 {}",
                spark(&state.prom.p50_hist),
                spark(&state.prom.p95_hist)
            ),
            Style::default().fg(ACCENT),
        )),
    ];
    if !state.prom.routes.is_empty() {
        lines.push(Line::from(Span::styled(
            "ROUTES",
            Style::default().fg(DIM).add_modifier(Modifier::BOLD),
        )));
        for r in &state.prom.routes {
            lines.push(Line::from(Span::styled(
                format!("{}  {}  {}", r.path, r.calls, r.ms),
                Style::default().fg(DIM),
            )));
        }
    }
    (format!("LENS  {}", svc.name), lines)
}

fn trace_lens(state: &TuiState) -> (String, Vec<Line<'static>>) {
    let id = state.selected_trace.as_deref().unwrap_or("—");
    let mut lines = Vec::new();
    let rows = flatten(&state.spans);
    let total = rows
        .first()
        .map(|(_, s)| s.duration_ms)
        .filter(|d| *d > 0.0)
        .unwrap_or(1.0);
    for (depth, sp) in rows {
        let indent = " ".repeat(depth);
        let w = ((sp.duration_ms / total) * 18.0).round().clamp(1.0, 18.0) as usize;
        let off = ((sp.start_ms / total) * 18.0).round().clamp(0.0, 17.0) as usize;
        let bar = format!("{}{}", " ".repeat(off), "█".repeat(w));
        let color = if sp.status == "error" { ERR } else { ACCENT };
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{indent}{:<16}",
                    truncate(&format!("{} {}", sp.service, sp.name), 16)
                ),
                Style::default().fg(color),
            ),
            Span::styled(bar, Style::default().fg(color)),
            Span::styled(
                format!(" {:>5.0}ms", sp.duration_ms),
                Style::default().fg(DIM),
            ),
        ]));
        if let Some(kind) = sp.attributes.get("kind") {
            lines.push(Line::from(Span::styled(
                format!("{indent}  kind={kind}"),
                Style::default().fg(DIM),
            )));
        }
        for ev in &sp.events {
            if let Some(sql) = ev.attributes.get("db.statement") {
                lines.push(Line::from(Span::styled(
                    format!("{indent}  {sql}"),
                    Style::default().fg(DIM),
                )));
            }
        }
    }
    if let Some(n1) = n1_insight(&state.spans) {
        lines.push(Line::from(Span::styled(
            format!("↳ {n1}"),
            Style::default().fg(WARN),
        )));
    }
    for l in state.loki_lines.iter().take(4) {
        if l.line.to_ascii_lowercase().contains("panic") {
            lines.push(Line::from(Span::styled(
                format!("{} {}", l.service, l.line),
                Style::default().fg(ERR),
            )));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "select a trace ⏎",
            Style::default().fg(DIM),
        )));
    }
    (format!("LENS  {id}"), lines)
}

fn mail_lens(state: &TuiState) -> (String, Vec<Line<'static>>) {
    let mut lines = Vec::new();
    if state.emails.is_empty() {
        lines.push(Line::from(Span::styled(
            "Outbox empty",
            Style::default().fg(DIM),
        )));
    }
    for m in state.emails.iter().take(12) {
        lines.push(Line::from(Span::styled(
            m.subject.clone(),
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(Span::styled(
            m.to.clone(),
            Style::default().fg(DIM),
        )));
    }
    (format!("LENS  mail ({})", state.emails.len()), lines)
}

fn jobs_lens(state: &TuiState) -> (String, Vec<Line<'static>>) {
    let mut lines = Vec::new();
    for j in state.jobs.iter().take(12) {
        let color = match j.status.as_str() {
            "failed" => ERR,
            "running" => WARN,
            _ => OK,
        };
        let ms = j
            .executions
            .first()
            .map(|e| format!(" {}ms {}", e.execution_time_ms, e.result))
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(j.job_type.clone(), Style::default().fg(Color::White)),
            Span::raw("  "),
            Span::styled(j.status.clone(), Style::default().fg(color)),
            Span::styled(ms, Style::default().fg(DIM)),
        ]));
        if let Some(e) = j.executions.iter().find(|e| e.failure_reason.is_some()) {
            if let Some(msg) = &e.failure_reason {
                lines.push(Line::from(Span::styled(
                    msg.clone(),
                    Style::default().fg(ERR),
                )));
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "no jobs",
            Style::default().fg(DIM),
        )));
    }
    (format!("LENS  jobs ({})", state.jobs.len()), lines)
}

fn render_wire(frame: &mut Frame, area: Rect, state: &TuiState) {
    let win = if state.wide_wire { "5m" } else { "30s" };
    let mut lines = vec![Line::from(Span::styled(
        format!("WIRE  · {win}  ▪ request  ◆ reload  ✗ error"),
        Style::default().fg(DIM),
    ))];
    let width = area.width.saturating_sub(10) as usize;
    for svc in &state.services {
        let lane = wire_lane(svc, state, width);
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<6}", truncate(&svc.name, 6)),
                Style::default().fg(DIM),
            ),
            Span::styled(lane, Style::default().fg(ACCENT)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn wire_lane(svc: &ServiceRow, state: &TuiState, width: usize) -> String {
    let mut cells = vec!['·'; width.max(1)];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let window = if state.wide_wire { 300.0 } else { 30.0 };
    for hit in &state.traces {
        if !trace_belongs_to(&hit.service, &svc.name) {
            continue;
        }
        let start = hit.start_unix_nano.parse::<f64>().unwrap_or(0.0) / 1e9;
        let age = now - start;
        if age < 0.0 || age > window {
            continue;
        }
        let x = ((1.0 - age / window) * (width.saturating_sub(1) as f64)).round() as usize;
        if x < cells.len() {
            cells[x] = if hit.duration_ms > 500.0 {
                '✗'
            } else {
                '▪'
            };
        }
    }
    for l in &state.logs {
        if l.label != svc.name {
            continue;
        }
        let lower = l.line.to_ascii_lowercase();
        if lower.contains("hmr") || lower.contains("reload") || lower.contains("rebuilt") {
            if let Some(c) = cells.last_mut() {
                *c = '◆';
            }
        }
    }
    cells.into_iter().collect()
}

fn render_footer(frame: &mut Frame, area: Rect, state: &TuiState, wide: bool) {
    let keys = if wide {
        "1-9 focus  0 all  ↑↓ log  ⏎ trace  r restart  o open  m/M migrate  e edit  p pause  w wire  q quit"
    } else {
        "1-9  r  o  m  q"
    };
    let mut spans = vec![Span::styled(keys, Style::default().fg(DIM))];
    if !state.toast.is_empty() {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            state.toast.clone(),
            Style::default().fg(ACCENT),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn fact_owned(k: &str, v: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:<6} "), Style::default().fg(DIM)),
        Span::styled(v, Style::default().fg(Color::Gray)),
    ])
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;
    use crate::commands::dev::banner::DevUrls;
    use crate::commands::dev::log::LogLine;
    use crate::commands::dev::tui::tempo::TraceHit;

    fn buffer_text(backend: &TestBackend) -> String {
        let buf = backend.buffer();
        let area = buf.area();
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn wide_dashboard_names_the_started_services() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        state.logs = vec![LogLine {
            label: "api".into(),
            line: "listening".into(),
        }];
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("erno dev"), "{text}");
        assert!(text.contains("teryon"), "{text}");
        for name in ["api", "app", "www", "prom", "tempo", "loki"] {
            assert!(text.contains(name), "missing {name} in:\n{text}");
        }
        assert!(text.contains("LOG"), "{text}");
        assert!(text.contains("LENS"), "{text}");
        assert!(text.contains("SERVICES"), "{text}");
        assert!(
            text.contains("npm run dev") || text.contains("cargo run"),
            "{text}"
        );
        assert!(text.contains("WIRE"), "{text}");
        assert!(text.contains("MIGRATIONS"), "{text}");
    }

    fn hit(service: &str, name: &str) -> TraceHit {
        TraceHit {
            trace_id: name.into(),
            name: name.into(),
            service: service.into(),
            duration_ms: 12.0,
            start_unix_nano: String::new(),
        }
    }

    #[test]
    fn focused_log_does_not_keep_other_services() {
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
                line: "api-only-line".into(),
            },
            LogLine {
                label: "app".into(),
                line: "app-only-line".into(),
            },
        ];
        state.traces = vec![hit("erno", "GET /secret"), hit("app", "vite-hmr")];
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("app-only-line"), "{text}");
        assert!(text.contains("vite-hmr"), "{text}");
        assert!(!text.contains("api-only-line"), "{text}");
        assert!(!text.contains("GET /secret"), "{text}");
    }

    fn service_index(state: &TuiState, name: &str) -> usize {
        state
            .services
            .iter()
            .position(|s| s.name == name)
            .unwrap_or_else(|| panic!("{name}"))
    }

    #[test]
    fn switching_focus_overwrites_the_previous_log_cells() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        state.focus = Some(service_index(&state, "api"));
        state.logs = (0..40)
            .map(|i| LogLine {
                label: "api".into(),
                line: format!("\x1b[31mUNIQUE-API-LINE-{i:02}\x1b[0m compiling leftover"),
            })
            .collect();
        state.traces = vec![hit("erno", "UNIQUE-API-TRACE")];
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let first = buffer_text(terminal.backend());
        assert!(first.contains("UNIQUE-API-LINE"), "{first}");

        state.focus = Some(service_index(&state, "app"));
        state.logs.push(LogLine {
            label: "app".into(),
            line: "app-only-after-switch".into(),
        });
        terminal.draw(|f| render(f, &state)).unwrap();
        let second = buffer_text(terminal.backend());
        assert!(second.contains("app-only-after-switch"), "{second}");
        assert!(
            !second.contains("UNIQUE-API-LINE"),
            "api log cells survived the switch:\n{second}"
        );
        assert!(
            !second.contains("UNIQUE-API-TRACE"),
            "api trace cells survived the switch:\n{second}"
        );
        assert!(!second.contains("compiling leftover"), "{second}");
    }
}
