use std::io;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::crossterm::ExecutableCommand;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Sparkline, Table, TableState},
    Frame, Terminal,
};
use tokio::runtime::Handle;
use uuid::Uuid;

use super::client::{
    AdminClient, DashboardResponse, JobSummary, JobTypeStat, MetricSeriesDto, SubscriptionInfo,
    UserDetailResponse, UserSummary,
};

const STATS_WINDOW_CHOICES: &[i64] = &[7, 30, 90];

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct DashboardData {
    data: Option<DashboardResponse>,
    loaded: bool,
}

#[derive(Default)]
struct UsersState {
    query: String,
    users: Vec<UserSummary>,
    table_state: TableState,
}

#[derive(Default)]
struct JobsState {
    stats: Vec<JobTypeStat>,
    jobs: Vec<JobSummary>,
    status_filter: Option<&'static str>,
    type_filter: Option<String>,
    top_state: TableState,
    bottom_state: TableState,
    panel: JobPanel,
}

#[derive(Default, PartialEq)]
enum JobPanel {
    Top,
    #[default]
    Bottom,
}

enum Screen {
    Dashboard,
    Users,
    UserDetail {
        detail: UserDetailResponse,
    },
    GiftSubscription {
        detail: UserDetailResponse,
        plan_idx: usize,
        days_input: String,
        confirming: bool,
    },
    DeleteConfirm {
        detail: UserDetailResponse,
        email_input: String,
    },
    Jobs,
    Stats,
}

#[derive(Default)]
struct StatsState {
    series: Vec<MetricSeriesDto>,
    window_days: i64,
    loaded: bool,
}

struct AdminApp {
    client: AdminClient,
    handle: Handle,
    plans: Vec<String>,
    screen: Screen,
    dashboard: DashboardData,
    users: UsersState,
    jobs: JobsState,
    stats: StatsState,
    message: Option<(String, bool)>,
    base_url: String,
}

impl AdminApp {
    fn new(client: AdminClient, handle: Handle, plans: Vec<String>, base_url: String) -> Self {
        Self {
            client,
            handle,
            plans,
            screen: Screen::Dashboard,
            dashboard: DashboardData::default(),
            users: UsersState::default(),
            jobs: JobsState::default(),
            stats: StatsState {
                window_days: STATS_WINDOW_CHOICES[0],
                ..Default::default()
            },
            message: None,
            base_url,
        }
    }

    fn block_on<F, T>(&self, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        self.handle.block_on(f)
    }

    fn load_dashboard(&mut self) {
        match self.block_on(self.client.dashboard()) {
            Ok(data) => {
                self.dashboard.data = Some(data);
                self.dashboard.loaded = true;
            }
            Err(e) => self.message = Some((format!("Failed to load dashboard: {e}"), true)),
        }
    }

    fn load_users(&mut self) {
        match self.block_on(self.client.list_users(&self.users.query)) {
            Ok(resp) => {
                self.users.users = resp.users;
                if self.users.users.is_empty() {
                    self.users.table_state.select(None);
                } else {
                    self.users.table_state.select(Some(0));
                }
            }
            Err(e) => self.message = Some((format!("Failed to load users: {e}"), true)),
        }
    }

    fn load_user_detail(&mut self, id: Uuid) {
        match self.block_on(self.client.get_user(id)) {
            Ok(detail) => self.screen = Screen::UserDetail { detail },
            Err(e) => self.message = Some((format!("Failed to load user: {e}"), true)),
        }
    }

    fn do_activate_user(&mut self, user_id: Uuid) {
        match self.block_on(self.client.activate_user(user_id)) {
            Ok(detail) => {
                self.message = Some(("User activated.".to_string(), false));
                self.screen = Screen::UserDetail { detail };
            }
            Err(e) => self.message = Some((format!("Failed to activate: {e}"), true)),
        }
    }

    fn do_delete_user(&mut self, user_id: Uuid) {
        match self.block_on(self.client.delete_user(user_id)) {
            Ok(()) => {
                self.message = Some(("User deleted.".to_string(), false));
                self.users.users.retain(|u| u.id != user_id);
                if self.users.users.is_empty() {
                    self.users.table_state.select(None);
                } else {
                    self.users.table_state.select(Some(0));
                }
                self.screen = Screen::Users;
            }
            Err(e) => {
                self.message = Some((format!("Failed to delete user: {e}"), true));
                self.screen = Screen::Users;
            }
        }
    }

    fn do_gift_subscription(&mut self, user_id: Uuid, plan: String, days: u32) {
        match self.block_on(self.client.gift_user(user_id, &plan, days)) {
            Ok(detail) => {
                self.message = Some(("Gift subscription created.".to_string(), false));
                self.screen = Screen::UserDetail { detail };
            }
            Err(e) => {
                self.message = Some((format!("Failed to gift subscription: {e}"), true));
                self.screen = Screen::Users;
            }
        }
    }

    fn load_jobs(&mut self) {
        match self.block_on(self.client.list_jobs(
            self.jobs.status_filter,
            self.jobs.type_filter.as_deref(),
        )) {
            Ok(resp) => {
                self.jobs.stats = resp.stats;
                self.jobs.jobs = resp.jobs;
                if !self.jobs.jobs.is_empty() && self.jobs.bottom_state.selected().is_none() {
                    self.jobs.bottom_state.select(Some(0));
                }
                if !self.jobs.stats.is_empty() && self.jobs.top_state.selected().is_none() {
                    self.jobs.top_state.select(Some(0));
                }
            }
            Err(e) => self.message = Some((format!("Failed to load jobs: {e}"), true)),
        }
    }

    fn do_retry_job(&mut self, job_id: Uuid) {
        match self.block_on(self.client.retry_job(job_id)) {
            Ok(()) => {
                self.message = Some(("Job queued for retry.".to_string(), false));
                self.load_jobs();
            }
            Err(e) => self.message = Some((format!("Failed to retry job: {e}"), true)),
        }
    }

    fn load_stats(&mut self) {
        match self.block_on(self.client.stats(self.stats.window_days)) {
            Ok(resp) => {
                self.stats.series = resp.series;
                self.stats.window_days = resp.window_days;
                self.stats.loaded = true;
            }
            Err(e) => self.message = Some((format!("Failed to load stats: {e}"), true)),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.message.is_some() {
            self.message = None;
        }
        match &self.screen {
            Screen::Dashboard => self.handle_key_dashboard(key),
            Screen::Users => self.handle_key_users(key),
            Screen::UserDetail { .. } => self.handle_key_user_detail(key),
            Screen::GiftSubscription { .. } => self.handle_key_gift(key),
            Screen::DeleteConfirm { .. } => self.handle_key_delete_confirm(key),
            Screen::Jobs => self.handle_key_jobs(key),
            Screen::Stats => self.handle_key_stats(key),
        }
    }

    fn handle_key_dashboard(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => return true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('u') | KeyCode::Char('U') => {
                self.screen = Screen::Users;
                self.load_users();
            }
            KeyCode::Char('j') | KeyCode::Char('J') => {
                self.screen = Screen::Jobs;
                self.load_jobs();
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.screen = Screen::Stats;
                self.load_stats();
            }
            KeyCode::Char('r') | KeyCode::Char('R') => self.load_dashboard(),
            _ => {}
        }
        false
    }

    fn handle_key_stats(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => self.screen = Screen::Dashboard,
            KeyCode::Char('w') | KeyCode::Char('W') => {
                let current = STATS_WINDOW_CHOICES
                    .iter()
                    .position(|&d| d == self.stats.window_days)
                    .unwrap_or(0);
                let next = (current + 1) % STATS_WINDOW_CHOICES.len();
                self.stats.window_days = STATS_WINDOW_CHOICES[next];
                self.load_stats();
            }
            KeyCode::Char('r') | KeyCode::Char('R') => self.load_stats(),
            _ => {}
        }
        false
    }

    fn handle_key_users(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => self.screen = Screen::Dashboard,
            KeyCode::Up => {
                let i = self.users.table_state.selected().unwrap_or(0);
                if i > 0 {
                    self.users.table_state.select(Some(i - 1));
                }
            }
            KeyCode::Down => {
                let len = self.users.users.len();
                if len > 0 {
                    let i = self.users.table_state.selected().unwrap_or(0);
                    if i + 1 < len {
                        self.users.table_state.select(Some(i + 1));
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(idx) = self.users.table_state.selected() {
                    if let Some(u) = self.users.users.get(idx) {
                        self.load_user_detail(u.id);
                    }
                }
            }
            KeyCode::Backspace => {
                self.users.query.pop();
                self.load_users();
            }
            KeyCode::Char(c) => {
                self.users.query.push(c);
                self.load_users();
            }
            _ => {}
        }
        false
    }

    fn handle_key_user_detail(&mut self, key: KeyEvent) -> bool {
        let Screen::UserDetail { ref detail } = self.screen else {
            return false;
        };
        let user_id = detail.user.id;
        let detail_clone = detail.clone();

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => self.screen = Screen::Users,
            KeyCode::Char('g') | KeyCode::Char('G') => {
                self.screen = Screen::GiftSubscription {
                    detail: detail_clone,
                    plan_idx: 0,
                    days_input: "30".to_string(),
                    confirming: false,
                };
            }
            KeyCode::Char('a') | KeyCode::Char('A') => self.do_activate_user(user_id),
            KeyCode::Char('x') | KeyCode::Char('X') => {
                self.screen = Screen::DeleteConfirm {
                    detail: detail_clone,
                    email_input: String::new(),
                };
            }
            _ => {}
        }
        false
    }

    fn handle_key_gift(&mut self, key: KeyEvent) -> bool {
        let Screen::GiftSubscription {
            ref detail,
            ref mut plan_idx,
            ref mut days_input,
            ref mut confirming,
        } = self.screen
        else {
            return false;
        };

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => {
                let id = detail.user.id;
                self.load_user_detail(id);
                return false;
            }
            KeyCode::Tab => *confirming = !*confirming,
            KeyCode::Up if !*confirming && *plan_idx > 0 => {
                *plan_idx -= 1;
            }
            KeyCode::Down
                if !*confirming
                    && !self.plans.is_empty()
                    && *plan_idx + 1 < self.plans.len() =>
            {
                *plan_idx += 1;
            }
            KeyCode::Backspace if *confirming => {
                days_input.pop();
            }
            KeyCode::Char(c) if *confirming && c.is_ascii_digit() => days_input.push(c),
            KeyCode::Enter => {
                let user_id = detail.user.id;
                let plan = self.plans.get(*plan_idx).cloned().unwrap_or_default();
                let days: u32 = days_input.parse().unwrap_or(30);
                if plan.is_empty() {
                    self.message = Some(("No plan selected.".to_string(), true));
                } else if days == 0 {
                    self.message = Some(("Days must be positive.".to_string(), true));
                } else {
                    self.do_gift_subscription(user_id, plan, days);
                }
            }
            _ => {}
        }
        false
    }

    fn handle_key_delete_confirm(&mut self, key: KeyEvent) -> bool {
        let Screen::DeleteConfirm {
            ref detail,
            ref mut email_input,
        } = self.screen
        else {
            return false;
        };

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => {
                let id = detail.user.id;
                self.load_user_detail(id);
                return false;
            }
            KeyCode::Backspace => {
                email_input.pop();
            }
            KeyCode::Char(c) => email_input.push(c),
            KeyCode::Enter => {
                if email_input == &detail.user.email {
                    let user_id = detail.user.id;
                    self.do_delete_user(user_id);
                } else {
                    self.message = Some((
                        "Email does not match. Deletion cancelled.".to_string(),
                        true,
                    ));
                    let id = detail.user.id;
                    self.load_user_detail(id);
                }
            }
            _ => {}
        }
        false
    }

    fn handle_key_jobs(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => self.screen = Screen::Dashboard,
            KeyCode::Tab => {
                self.jobs.panel = match self.jobs.panel {
                    JobPanel::Top => JobPanel::Bottom,
                    JobPanel::Bottom => JobPanel::Top,
                };
            }
            KeyCode::Up => match self.jobs.panel {
                JobPanel::Top => {
                    let i = self.jobs.top_state.selected().unwrap_or(0);
                    if i > 0 {
                        self.jobs.top_state.select(Some(i - 1));
                    }
                }
                JobPanel::Bottom => {
                    let i = self.jobs.bottom_state.selected().unwrap_or(0);
                    if i > 0 {
                        self.jobs.bottom_state.select(Some(i - 1));
                    }
                }
            },
            KeyCode::Down => match self.jobs.panel {
                JobPanel::Top => {
                    let len = self.jobs.stats.len();
                    if len > 0 {
                        let i = self.jobs.top_state.selected().unwrap_or(0);
                        if i + 1 < len {
                            self.jobs.top_state.select(Some(i + 1));
                        }
                    }
                }
                JobPanel::Bottom => {
                    let len = self.jobs.jobs.len();
                    if len > 0 {
                        let i = self.jobs.bottom_state.selected().unwrap_or(0);
                        if i + 1 < len {
                            self.jobs.bottom_state.select(Some(i + 1));
                        }
                    }
                }
            },
            KeyCode::Char('f') | KeyCode::Char('F') => {
                self.jobs.status_filter = match self.jobs.status_filter {
                    None => Some("Failed"),
                    Some("Failed") => Some("Pending"),
                    Some("Pending") => Some("Running"),
                    Some("Running") => None,
                    _ => None,
                };
                self.load_jobs();
            }
            KeyCode::Char('t') | KeyCode::Char('T') if self.jobs.panel == JobPanel::Top => {
                if let Some(idx) = self.jobs.top_state.selected() {
                    if let Some(stat) = self.jobs.stats.get(idx) {
                        let t = stat.job_type.clone();
                        self.jobs.type_filter = if self.jobs.type_filter.as_deref() == Some(&t) {
                            None
                        } else {
                            Some(t)
                        };
                        self.load_jobs();
                    }
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.jobs.panel == JobPanel::Bottom {
                    if let Some(idx) = self.jobs.bottom_state.selected() {
                        if let Some(j) = self.jobs.jobs.get(idx) {
                            if j.status == "Failed" || j.status == "failed" {
                                let id = j.id;
                                self.do_retry_job(id);
                            } else {
                                self.message =
                                    Some(("Only Failed jobs can be retried.".to_string(), true));
                            }
                        }
                    }
                } else {
                    self.load_jobs();
                }
            }
            _ => {}
        }
        false
    }

    fn render(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(f.area());

        match &self.screen {
            Screen::Dashboard => self.render_dashboard(f, chunks[0]),
            Screen::Users => self.render_users(f, chunks[0]),
            Screen::UserDetail { .. } => self.render_user_detail(f, chunks[0]),
            Screen::GiftSubscription { .. } => self.render_gift(f, chunks[0]),
            Screen::DeleteConfirm { .. } => self.render_delete_confirm(f, chunks[0]),
            Screen::Jobs => self.render_jobs(f, chunks[0]),
            Screen::Stats => self.render_stats(f, chunks[0]),
        }

        let status_text = if let Some((ref msg, is_err)) = self.message {
            let style = if is_err {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };
            Paragraph::new(Line::from(Span::styled(format!(" {msg}"), style)))
        } else {
            Paragraph::new("")
        };
        f.render_widget(status_text, chunks[1]);
    }

    fn render_dashboard(&self, f: &mut Frame, area: Rect) {
        let title = format!(" Erno Admin ── {} ", self.base_url);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_bottom(" [u] Users  [j] Jobs  [s] Stats  [r] Refresh  [q] Quit ");

        let inner = block.inner(area);
        f.render_widget(block, area);

        let section_label_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);

        let Some(d) = &self.dashboard.data else {
            f.render_widget(
                Paragraph::new(" Loading… ").alignment(Alignment::Center),
                inner,
            );
            return;
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(5),
                Constraint::Length(1),
                Constraint::Length(5),
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(inner);

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" USERS", section_label_style))),
            chunks[0],
        );
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" JOBS", section_label_style))),
            chunks[2],
        );
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" EMAILS", section_label_style))),
            chunks[4],
        );

        let user_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ])
            .split(chunks[1]);

        metric_box(f, user_row[0], "Total", &d.total_users.to_string(), Color::White);
        metric_box(f, user_row[1], "Stripe", &d.stripe_active.to_string(), Color::Blue);
        metric_box(f, user_row[2], "Gift", &d.gift_active.to_string(), Color::Magenta);
        metric_box(f, user_row[3], "Trial", &d.trial_active.to_string(), Color::Yellow);
        metric_box(f, user_row[4], "None", &d.no_sub.to_string(), Color::DarkGray);

        let job_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(chunks[3]);

        metric_box(f, job_row[0], "Pending", &d.pending_jobs.to_string(), Color::Yellow);
        metric_box(f, job_row[1], "Running", &d.running_jobs.to_string(), Color::Cyan);
        metric_box(f, job_row[2], "Failed", &d.failed_jobs.to_string(), Color::Red);
        metric_box(
            f,
            job_row[3],
            "Avg ms (1h)",
            &d.avg_execution_ms.to_string(),
            Color::Green,
        );

        let email_lines: Vec<Line> = if d.email_stats.is_empty() {
            vec![Line::from("  (no email jobs in retention window)")]
        } else {
            d.email_stats
                .iter()
                .map(|e| {
                    Line::from(format!(
                        "  {:20} total={:<6} ok={:<6} fail={}",
                        e.name, e.total, e.completed, e.failed
                    ))
                })
                .collect()
        };
        f.render_widget(
            Paragraph::new(email_lines).block(Block::default().borders(Borders::NONE)),
            chunks[5],
        );

        let refreshed = format!(" refreshed {}", d.refreshed_at.format("%H:%M:%S"));
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                refreshed,
                Style::default().fg(Color::DarkGray),
            ))),
            chunks[6],
        );
    }

    fn render_users(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);

        let search = Paragraph::new(self.users.query.clone()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Search (type to filter) "),
        );
        f.render_widget(search, chunks[0]);

        let rows: Vec<Row> = self
            .users
            .users
            .iter()
            .map(|u| {
                let verified = if u.email_verified_at.is_some() {
                    "yes"
                } else {
                    "no"
                };
                let plan = u
                    .subscription_plan
                    .as_deref()
                    .or(u.subscription_type.as_deref())
                    .unwrap_or("-");
                Row::new(vec![
                    Cell::from(u.email.clone()),
                    Cell::from(verified),
                    Cell::from(plan.to_string()),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(55),
                Constraint::Percentage(15),
                Constraint::Percentage(30),
            ],
        )
        .header(
            Row::new(vec!["Email", "Verified", "Plan"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(Block::default().borders(Borders::ALL).title(" Users "))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("► ");

        f.render_stateful_widget(table, chunks[1], &mut self.users.table_state);

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " ↑↓ Navigate  Enter Detail  Esc Dashboard ",
                Style::default().fg(Color::DarkGray),
            ))),
            chunks[2],
        );
    }

    fn render_user_detail(&self, f: &mut Frame, area: Rect) {
        let Screen::UserDetail { ref detail } = self.screen else {
            return;
        };
        let u = &detail.user;

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),
                Constraint::Min(5),
                Constraint::Length(1),
            ])
            .split(area);

        let verified = u
            .email_verified_at
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "not verified".to_string());

        let user_text = vec![
            Line::from(format!("  Email:     {}", u.email)),
            Line::from(format!("  ID:        {}", u.id)),
            Line::from(format!("  Verified:  {verified}")),
            Line::from(format!(
                "  Created:   {}",
                u.created_at.format("%Y-%m-%d %H:%M")
            )),
        ];
        f.render_widget(
            Paragraph::new(user_text).block(Block::default().borders(Borders::ALL).title(" User ")),
            chunks[0],
        );

        let sub_text = match &detail.subscription {
            Some(s) => subscription_lines(s),
            None => vec![Line::from("  No active subscription")],
        };
        f.render_widget(
            Paragraph::new(sub_text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Subscription "),
            ),
            chunks[1],
        );

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " [g] Gift  [a] Activate  [x] Delete  Esc Back ",
                Style::default().fg(Color::DarkGray),
            ))),
            chunks[2],
        );
    }

    fn render_gift(&self, f: &mut Frame, area: Rect) {
        let Screen::GiftSubscription {
            ref detail,
            plan_idx,
            ref days_input,
            confirming,
        } = self.screen
        else {
            return;
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        f.render_widget(
            Paragraph::new(format!(" Gift subscription to {}", detail.user.email)).block(
                Block::default().borders(Borders::ALL).title(" Gift "),
            ),
            chunks[0],
        );

        let plan_items: Vec<ListItem> = if self.plans.is_empty() {
            vec![ListItem::new("  (no plans configured in Stripe)")]
        } else {
            self.plans
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    if i == plan_idx {
                        ListItem::new(format!("  ► {p}"))
                    } else {
                        ListItem::new(format!("    {p}"))
                    }
                })
                .collect()
        };
        let plan_style = if confirming {
            Style::default()
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        f.render_widget(
            List::new(plan_items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Plan ")
                    .border_style(plan_style),
            ),
            chunks[1],
        );

        let days_style = if confirming {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        f.render_widget(
            Paragraph::new(days_input.clone()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Duration (days) ")
                    .border_style(days_style),
            ),
            chunks[2],
        );

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Tab focus  ↑↓ plan  Enter confirm  Esc cancel ",
                Style::default().fg(Color::DarkGray),
            ))),
            chunks[3],
        );
    }

    fn render_delete_confirm(&self, f: &mut Frame, area: Rect) {
        let Screen::DeleteConfirm {
            ref detail,
            ref email_input,
        } = self.screen
        else {
            return;
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "  PERMANENTLY DELETE USER",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(format!("  Type the email to confirm: {}", detail.user.email)),
            ])
            .block(Block::default().borders(Borders::ALL).title(" Confirm ")),
            chunks[0],
        );

        f.render_widget(
            Paragraph::new(email_input.clone())
                .block(Block::default().borders(Borders::ALL).title(" Email ")),
            chunks[1],
        );

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Enter delete  Esc cancel ",
                Style::default().fg(Color::DarkGray),
            ))),
            chunks[2],
        );
    }

    fn render_jobs(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Percentage(55),
                Constraint::Length(1),
            ])
            .split(area);

        let top_border = if self.jobs.panel == JobPanel::Top {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let bottom_border = if self.jobs.panel == JobPanel::Bottom {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };

        let filter_label = format!(
            " Stats  filter={}  type={} ",
            self.jobs.status_filter.unwrap_or("all"),
            self.jobs.type_filter.as_deref().unwrap_or("all")
        );

        let stats_rows: Vec<Row> = self
            .jobs
            .stats
            .iter()
            .map(|s| {
                Row::new(vec![
                    Cell::from(s.job_type.clone()),
                    Cell::from(s.pending.to_string()),
                    Cell::from(s.running.to_string()),
                    Cell::from(s.failed.to_string()),
                    Cell::from(s.completed.to_string()),
                ])
            })
            .collect();

        let stats_table = Table::new(
            stats_rows,
            [
                Constraint::Percentage(40),
                Constraint::Percentage(15),
                Constraint::Percentage(15),
                Constraint::Percentage(15),
                Constraint::Percentage(15),
            ],
        )
        .header(
            Row::new(vec!["Type", "Pend", "Run", "Fail", "Done"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(filter_label)
                .border_style(top_border),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("► ");
        f.render_stateful_widget(stats_table, chunks[0], &mut self.jobs.top_state);

        let job_rows: Vec<Row> = self
            .jobs
            .jobs
            .iter()
            .map(|j| {
                Row::new(vec![
                    Cell::from(j.job_type.clone()),
                    Cell::from(j.status.clone()),
                    Cell::from(j.retry_count.to_string()),
                    Cell::from(j.created_at.format("%m-%d %H:%M").to_string()),
                ])
            })
            .collect();

        let jobs_table = Table::new(
            job_rows,
            [
                Constraint::Percentage(40),
                Constraint::Percentage(20),
                Constraint::Percentage(10),
                Constraint::Percentage(30),
            ],
        )
        .header(
            Row::new(vec!["Type", "Status", "Retries", "Created"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Jobs ")
                .border_style(bottom_border),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("► ");
        f.render_stateful_widget(jobs_table, chunks[1], &mut self.jobs.bottom_state);

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Tab panel  f status  t type  r retry/refresh  Esc Dashboard ",
                Style::default().fg(Color::DarkGray),
            ))),
            chunks[2],
        );
    }

    fn render_stats(&self, f: &mut Frame, area: Rect) {
        let outer = Block::default()
            .title(format!(
                " Business Stats ── last {} days ",
                self.stats.window_days
            ))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = outer.inner(area);
        f.render_widget(outer, area);

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(inner);

        if !self.stats.loaded || self.stats.series.is_empty() {
            let text = if self.stats.loaded {
                "No snapshots yet. The business_stats_snapshot job runs daily — see \
                 docs for registering/scheduling it and listing it in a worker pool."
            } else {
                "Loading..."
            };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    text,
                    Style::default().fg(Color::DarkGray),
                ))),
                sections[0],
            );
        } else {
            let row_constraints: Vec<Constraint> = self
                .stats
                .series
                .iter()
                .map(|_| Constraint::Length(3))
                .collect();
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints(row_constraints)
                .split(sections[0]);

            for (series, row_area) in self.stats.series.iter().zip(rows.iter()) {
                render_metric_row(f, *row_area, series);
            }
        }

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  [w] Window   [r] Refresh   [Esc] Back",
                Style::default().fg(Color::DarkGray),
            ))),
            sections[1],
        );
    }
}

fn render_metric_row(f: &mut Frame, area: Rect, series: &MetricSeriesDto) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(26), Constraint::Min(0)])
        .split(area);

    let latest = series
        .points
        .last()
        .map(|p| format_stat_value(p.value))
        .unwrap_or_else(|| "—".to_string());

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                series.label.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw(latest)),
        ]),
        cols[0],
    );

    let data: Vec<u64> = series
        .points
        .iter()
        .map(|p| p.value.round().max(0.0) as u64)
        .collect();

    f.render_widget(
        Sparkline::default()
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Cyan))
            .data(&data),
        cols[1],
    );
}

fn format_stat_value(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

fn subscription_lines(s: &SubscriptionInfo) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("  Type:    {}", s.sub_type)),
        Line::from(format!("  Plan:    {}", s.plan)),
        Line::from(format!("  Status:  {}", s.status)),
        Line::from(format!("  Expiry:  {}", s.expiry)),
    ];
    if let Some(ref c) = s.stripe_customer_id {
        lines.push(Line::from(format!("  Customer: {c}")));
    }
    if let Some(ref sub) = s.stripe_sub_id {
        lines.push(Line::from(format!("  Sub ID:   {sub}")));
    }
    if let Some(cancel) = s.cancel_at_period_end {
        lines.push(Line::from(format!("  Cancel:   {cancel}")));
    }
    lines
}

fn metric_box(f: &mut Frame, area: Rect, label: &str, value: &str, color: Color) {
    let block = Block::default().borders(Borders::ALL).title(format!(" {label} "));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(value)
            .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center),
        inner,
    );
}

/// Run the admin TUI. Blocks until the user quits.
pub fn run(
    client: AdminClient,
    handle: &Handle,
    plans: Vec<String>,
    base_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_inner(&mut terminal, client, handle, plans, base_url);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_inner(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: AdminClient,
    handle: &Handle,
    plans: Vec<String>,
    base_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = AdminApp::new(client, handle.clone(), plans, base_url);
    app.load_dashboard();

    loop {
        terminal.draw(|f| app.render(f))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if app.handle_key(key) {
                    break;
                }
            }
        }
    }

    Ok(())
}
