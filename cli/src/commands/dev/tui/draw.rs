use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::state::{log_window, LensMode, ServiceRow, TuiState};

const ACCENT: Color = Color::Rgb(145, 132, 217);
const DIM: Color = Color::Rgb(117, 121, 140);
const OK: Color = Color::Rgb(94, 184, 158);
const WARN: Color = Color::Rgb(201, 168, 92);
const ERR: Color = Color::Rgb(196, 92, 78);

/// The wire is a footer strip of request lanes. It does not need the wide
/// three-column layout — only enough rows, which the TUI already requires.
fn show_wire(area: Rect) -> bool {
    area.height >= 24
}

fn split_layout(area: Rect) -> (Rect, Rect, Option<Rect>, Rect) {
    let wire = show_wire(area);
    let mut vert = vec![Constraint::Length(3), Constraint::Min(8)];
    if wire {
        vert.push(Constraint::Length(8));
    }
    vert.push(Constraint::Length(2));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vert)
        .split(area);
    let wire_rect = if wire { Some(chunks[2]) } else { None };
    let footer = chunks[chunks.len() - 1];
    (chunks[0], chunks[1], wire_rect, footer)
}

/// Inner rows of the LOG pane for `area`. Kept in lockstep with `render`.
pub fn log_inner_height(width: u16, height: u16) -> usize {
    let area = Rect {
        x: 0,
        y: 0,
        width,
        height,
    };
    split_layout(area).1.height.saturating_sub(2) as usize
}

pub fn render(frame: &mut Frame, state: &TuiState) {
    frame.render_widget(Clear, frame.area());
    let area = frame.area();
    let (header, body, wire, footer) = split_layout(area);
    render_header(frame, header, state);

    let wide = area.width >= 152;
    let mid = area.width >= 100;
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

    if let Some(wire) = wire {
        render_wire(frame, wire, state);
    }
    render_footer(frame, footer, state, wide);
}

fn render_header(frame: &mut Frame, area: Rect, state: &TuiState) {
    let ready = state
        .services
        .iter()
        .filter(|s| s.state.label() == "ready")
        .count();
    let status = format!("{ready}/{} up", state.services.len());
    let title = Line::from(vec![
        Span::styled("◈ erno dev", Style::default().fg(ACCENT)),
        Span::raw("  "),
        Span::styled(&state.project, Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(status, Style::default().fg(OK)),
        Span::raw("  "),
        Span::styled(state.elapsed(), Style::default().fg(DIM)),
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
    for l in state.visible_logs() {
        rows.push(log_row(&l.label, &l.line, None, inner_w));
    }
    let (start, end) = log_window(rows.len(), inner_h, state.log_offset);
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
    let lines = vec![
        Line::from(Span::styled(
            svc.name.clone(),
            Style::default().fg(Color::White),
        )),
        fact_owned("cmd", svc.cmd.clone()),
        fact_owned("pid", pid),
        fact_owned("watch", svc.watch.clone()),
        fact_owned("state", svc.state.label().to_string()),
        fact_owned("url", svc.url.clone()),
    ];
    (format!("LENS  {}", svc.name), lines)
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
    mark_wire_ticks(&mut cells, state.ticks(&svc.name), now, window);
    cells.into_iter().collect()
}

fn render_footer(frame: &mut Frame, area: Rect, state: &TuiState, wide: bool) {
    let keys = if wide {
        "1-9 focus  0 all  ↑↓ log  c copy  r restart  o open  m/M migrate  p pause  w wire  q quit"
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

fn mark_wire_ticks(cells: &mut [char], ticks: &[super::state::WireTick], now: f64, window: f64) {
    if cells.is_empty() || window <= 0.0 {
        return;
    }
    let last = cells.len() - 1;
    for tick in ticks {
        let age = now - tick.at;
        if age < 0.0 || age > window {
            continue;
        }
        let x = ((1.0 - age / window) * last as f64).round() as usize;
        if x > last {
            continue;
        }
        let mark = if tick.error {
            '✗'
        } else if tick.reload {
            '◆'
        } else {
            '▪'
        };
        if cells[x] == '·' || mark == '✗' || (mark == '◆' && cells[x] == '▪') {
            cells[x] = mark;
        }
    }
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
    use crate::commands::dev::tui::state::WireTick;

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
        state.logs = vec![LogLine::new("api", "listening")];
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("erno dev"), "{text}");
        assert!(text.contains("teryon"), "{text}");
        for name in ["api", "app", "www"] {
            assert!(text.contains(name), "missing {name} in:\n{text}");
        }
        assert!(text.contains("LOG"), "{text}");
        assert!(text.contains("LENS"), "{text}");
        assert!(text.contains("SERVICES"), "{text}");
        assert!(
            text.contains("bun run dev") || text.contains("cargo run"),
            "{text}"
        );
        assert!(text.contains("WIRE"), "{text}");
        assert!(text.contains("MIGRATIONS"), "{text}");
        assert!(text.contains("c copy"), "{text}");
    }
    #[test]
    fn log_offset_does_not_drop_lines_that_fit() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        state.logs = vec![
            LogLine::new("api", "UNIQUE-FIRST"),
            LogLine::new("api", "UNIQUE-MIDDLE"),
            LogLine::new("api", "UNIQUE-LAST"),
        ];
        state.log_offset = 2;
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("UNIQUE-FIRST"), "{text}");
        assert!(text.contains("UNIQUE-MIDDLE"), "{text}");
        assert!(
            text.contains("UNIQUE-LAST"),
            "↑ hid a line that still fit:\n{text}"
        );
    }

    #[test]
    fn overflowing_log_scrolls_newest_out_from_the_bottom() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        let h = log_inner_height(160, 40);
        assert!(h > 4, "expected a tall log pane, got {h}");
        let n = h + 3;
        state.logs = (0..n)
            .map(|i| LogLine::new("api", format!("OVERFLOW-LINE-{i:02}")))
            .collect();
        let newest = format!("OVERFLOW-LINE-{:02}", n - 1);
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let follow = buffer_text(terminal.backend());
        assert!(follow.contains(&newest), "{follow}");
        assert!(
            !follow.contains("OVERFLOW-LINE-00"),
            "follow should hide the oldest:\n{follow}"
        );

        state.log_offset = 1;
        terminal.draw(|f| render(f, &state)).unwrap();
        let scrolled = buffer_text(terminal.backend());
        assert!(
            !scrolled.contains(&newest),
            "newest should leave the bottom on ↑:\n{scrolled}"
        );
        assert!(
            scrolled.contains("OVERFLOW-LINE-02"),
            "older lines should enter from the top:\n{scrolled}"
        );
    }

    #[test]
    fn wire_is_drawn_on_a_standard_tty() {
        let urls = DevUrls::defaults(true, true, true);
        let state = TuiState::new("teryon", &urls);
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("WIRE"), "wire missing on 100×24:\n{text}");
        assert!(text.contains("30s"), "{text}");
        assert!(text.contains('·') || text.contains("·"), "{text}");
    }

    #[test]
    fn api_wire_ticks_mark_the_lane() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        state
            .wire_ticks
            .entry("api".into())
            .or_default()
            .push(WireTick {
                at: now,
                error: false,
                reload: false,
            });
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(
            text.contains('▪'),
            "api log tick should mark the wire:\n{text}"
        );
    }

    #[test]
    fn www_access_ticks_mark_the_lane() {
        let urls = DevUrls::defaults(true, true, true);
        let mut state = TuiState::new("teryon", &urls);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        state
            .wire_ticks
            .entry("www".into())
            .or_default()
            .push(WireTick {
                at: now,
                error: false,
                reload: false,
            });
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(
            text.contains('▪'),
            "www access log should mark the wire:\n{text}"
        );
    }
}
