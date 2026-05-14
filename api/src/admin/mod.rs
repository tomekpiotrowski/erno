//! Docs: docs/src/content/docs/api/console.md
use std::io;
use std::time::Duration;

use chrono::Utc;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::crossterm::ExecutableCommand;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, TableState,
    },
    Frame, Terminal,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use tokio::runtime::Handle;
use uuid::Uuid;

use crate::{
    billing::{
        handlers::webhooks::update_user_subscription_cache,
        lookup::{load_current_subscription, CurrentSubscription},
        models::gift_subscription,
    },
    database::models::{
        job::{self, Column as JobColumn},
        job_status::JobStatus,
        user::{self, Column as UserColumn},
    },
};

// ── Screen state ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct DashboardData {
    total_users: i64,
    stripe_active: i64,
    gift_active: i64,
    trial_active: i64,
    no_sub: i64,
    pending_jobs: i64,
    running_jobs: i64,
    failed_jobs: i64,
    loaded: bool,
}

struct UsersState {
    query: String,
    users: Vec<user::Model>,
    table_state: TableState,
}

impl Default for UsersState {
    fn default() -> Self {
        Self {
            query: String::new(),
            users: Vec::new(),
            table_state: TableState::default(),
        }
    }
}

struct JobsState {
    stats: Vec<(String, i64, i64, i64, i64)>, // type, pending, running, failed, completed
    jobs: Vec<job::Model>,
    status_filter: Option<JobStatus>,
    type_filter: Option<String>,
    top_state: TableState,
    bottom_state: TableState,
    panel: JobPanel,
}

impl Default for JobsState {
    fn default() -> Self {
        Self {
            stats: Vec::new(),
            jobs: Vec::new(),
            status_filter: None,
            type_filter: None,
            top_state: TableState::default(),
            bottom_state: TableState::default(),
            panel: JobPanel::Bottom,
        }
    }
}

#[derive(Default, PartialEq)]
enum JobPanel {
    Top,
    #[default]
    Bottom,
}

// ── Main screen enum ──────────────────────────────────────────────────────────

enum Screen {
    Dashboard,
    Users,
    UserDetail {
        user: user::Model,
        subscription: Option<SubInfo>,
    },
    GiftSubscription {
        user: user::Model,
        plan_idx: usize,
        days_input: String,
        confirming: bool,
    },
    DeleteConfirm {
        user: user::Model,
        email_input: String,
    },
    Jobs,
}

struct SubInfo {
    sub_type: String,
    plan: String,
    status: String,
    expiry: String,
    stripe_customer_id: Option<String>,
    stripe_sub_id: Option<String>,
    cancel_at_period_end: Option<bool>,
}

// ── App ───────────────────────────────────────────────────────────────────────

struct AdminApp<'a> {
    db: &'a DatabaseConnection,
    plans: &'a [String],
    handle: &'a Handle,
    screen: Screen,
    dashboard: DashboardData,
    users: UsersState,
    jobs: JobsState,
    message: Option<(String, bool)>, // (text, is_error)
}

impl<'a> AdminApp<'a> {
    fn new(db: &'a DatabaseConnection, plans: &'a [String], handle: &'a Handle) -> Self {
        Self {
            db,
            plans,
            handle,
            screen: Screen::Dashboard,
            dashboard: DashboardData::default(),
            users: UsersState::default(),
            jobs: JobsState::default(),
            message: None,
        }
    }

    // ── Data loading ──────────────────────────────────────────────────────────

    fn load_dashboard(&mut self) {
        use sea_orm::{DbBackend, Statement};

        let db = self.db;
        let result = self.handle.block_on(async {
            let count_from = |sql: &'static str| async move {
                db.query_one(Statement::from_string(DbBackend::Postgres, sql))
                    .await
                    .map(|r| r.and_then(|r| r.try_get::<i64>("", "count").ok()).unwrap_or(0))
            };

            let total = count_from("SELECT COUNT(*)::bigint AS count FROM users").await?;
            let stripe = count_from("SELECT COUNT(*)::bigint AS count FROM users WHERE subscription_type = 'stripe'").await?;
            let gift = count_from("SELECT COUNT(*)::bigint AS count FROM users WHERE subscription_type = 'gift'").await?;
            let trial = count_from("SELECT COUNT(*)::bigint AS count FROM users WHERE subscription_type = 'trial'").await?;
            let no_sub = count_from("SELECT COUNT(*)::bigint AS count FROM users WHERE subscription_type IS NULL").await?;
            let pending = count_from("SELECT COUNT(*)::bigint AS count FROM job WHERE status IN ('pending', 'pending_retry')").await?;
            let running = count_from("SELECT COUNT(*)::bigint AS count FROM job WHERE status = 'running'").await?;
            let failed = count_from("SELECT COUNT(*)::bigint AS count FROM job WHERE status = 'failed'").await?;

            Ok::<_, sea_orm::DbErr>((total, stripe, gift, trial, no_sub, pending, running, failed))
        });

        match result {
            Ok((total, stripe, gift, trial, no_sub, pending, running, failed)) => {
                self.dashboard = DashboardData {
                    total_users: total,
                    stripe_active: stripe,
                    gift_active: gift,
                    trial_active: trial,
                    no_sub,
                    pending_jobs: pending,
                    running_jobs: running,
                    failed_jobs: failed,
                    loaded: true,
                };
            }
            Err(e) => {
                self.message = Some((format!("Failed to load dashboard: {e}"), true));
            }
        }
    }

    fn load_users(&mut self) {
        let query = self.users.query.clone();
        let db = self.db;
        let result = self.handle.block_on(async {
            let mut q = user::Entity::find().order_by_asc(UserColumn::Email);
            if !query.is_empty() {
                q = q.filter(
                    UserColumn::Email.like(format!("%{}%", query.to_lowercase())),
                );
            }
            q.limit(200).all(db).await
        });

        match result {
            Ok(users) => {
                self.users.users = users;
                if self.users.users.is_empty() {
                    self.users.table_state.select(None);
                } else {
                    self.users.table_state.select(Some(0));
                }
            }
            Err(e) => {
                self.message = Some((format!("Failed to load users: {e}"), true));
            }
        }
    }

    fn load_user_detail(&mut self, u: user::Model) {
        let db = self.db;
        let sub_info = self.handle.block_on(async {
            load_current_subscription(db, &u).await
        });

        let subscription = sub_info.map(|s| match s {
            CurrentSubscription::Stripe(m) => SubInfo {
                sub_type: "Stripe".to_string(),
                plan: m.plan.clone(),
                status: format!("{:?}", m.status),
                expiry: m.current_period_end.format("%Y-%m-%d").to_string(),
                stripe_customer_id: Some(m.stripe_customer_id),
                stripe_sub_id: Some(m.stripe_subscription_id),
                cancel_at_period_end: Some(m.cancel_at_period_end),
            },
            CurrentSubscription::Gift(m) => SubInfo {
                sub_type: "Gift".to_string(),
                plan: m.plan,
                status: "Active".to_string(),
                expiry: m.active_until.format("%Y-%m-%d").to_string(),
                stripe_customer_id: None,
                stripe_sub_id: None,
                cancel_at_period_end: None,
            },
            CurrentSubscription::Trial(m) => SubInfo {
                sub_type: "Trial".to_string(),
                plan: m.plan,
                status: "Active".to_string(),
                expiry: m.active_until.format("%Y-%m-%d").to_string(),
                stripe_customer_id: None,
                stripe_sub_id: None,
                cancel_at_period_end: None,
            },
        });

        self.screen = Screen::UserDetail {
            user: u,
            subscription,
        };
    }

    fn do_activate_user(&mut self, user_id: Uuid) {
        let db = self.db;
        let result = self.handle.block_on(async {
            let now = Utc::now().naive_utc();
            let active = user::ActiveModel {
                id: Set(user_id),
                email_verified_at: Set(Some(now)),
                ..Default::default()
            };
            user::Entity::update(active).exec(db).await
        });

        match result {
            Ok(updated) => {
                self.message = Some(("User activated.".to_string(), false));
                // Reload detail with updated user
                self.load_user_detail(updated);
            }
            Err(e) => {
                self.message = Some((format!("Failed to activate: {e}"), true));
            }
        }
    }

    fn do_delete_user(&mut self, user_id: Uuid) {
        let db = self.db;
        let result = self.handle.block_on(async {
            user::Entity::delete_by_id(user_id).exec(db).await
        });

        match result {
            Ok(_) => {
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

    fn do_gift_subscription(&mut self, user_id: Uuid, plan: String, days: i64) {
        let db = self.db;
        let result = self.handle.block_on(async {
            let active_until = Utc::now().naive_utc() + chrono::Duration::days(days);
            let now = Utc::now().naive_utc();
            let row = gift_subscription::ActiveModel {
                user_id: Set(user_id),
                plan: Set(plan.clone()),
                active_until: Set(active_until),
                created_at: Set(now),
                ..Default::default()
            };
            let inserted = row.insert(db).await?;
            update_user_subscription_cache(
                db,
                user_id,
                Some(inserted.id),
                Some("gift".to_string()),
                Some(plan),
            )
            .await?;
            // Reload updated user
            user::Entity::find_by_id(user_id).one(db).await
        });

        match result {
            Ok(Some(updated_user)) => {
                self.message = Some(("Gift subscription created.".to_string(), false));
                self.load_user_detail(updated_user);
            }
            Ok(None) => {
                self.message = Some(("User not found after gifting.".to_string(), true));
                self.screen = Screen::Users;
            }
            Err(e) => {
                self.message = Some((format!("Failed to gift subscription: {e}"), true));
                // Navigate back to user detail
                self.screen = Screen::Users;
            }
        }
    }

    fn load_jobs(&mut self) {
        use sea_orm::{DbBackend, Statement};

        let db = self.db;
        let status_filter = self.jobs.status_filter.clone();
        let type_filter = self.jobs.type_filter.clone();

        let result = self.handle.block_on(async {
            // Load grouped stats
            let stats_rows = db
                .query_all(Statement::from_string(
                    DbBackend::Postgres,
                    "SELECT type, \
                     SUM(CASE WHEN status IN ('pending','pending_retry') THEN 1 ELSE 0 END) AS pending, \
                     SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END) AS running, \
                     SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed, \
                     SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) AS completed \
                     FROM job GROUP BY type ORDER BY type",
                ))
                .await?;

            let stats: Vec<(String, i64, i64, i64, i64)> = stats_rows
                .into_iter()
                .map(|r| {
                    let t: String = r.try_get("", "type").unwrap_or_default();
                    let p: i64 = r.try_get("", "pending").unwrap_or(0);
                    let ru: i64 = r.try_get("", "running").unwrap_or(0);
                    let f: i64 = r.try_get("", "failed").unwrap_or(0);
                    let c: i64 = r.try_get("", "completed").unwrap_or(0);
                    (t, p, ru, f, c)
                })
                .collect();

            // Load job list
            let mut q = job::Entity::find().order_by_desc(JobColumn::CreatedAt);
            if let Some(ref s) = status_filter {
                q = q.filter(JobColumn::Status.eq(s.clone()));
            }
            if let Some(ref t) = type_filter {
                q = q.filter(JobColumn::Type.eq(t.clone()));
            }
            let jobs = q.limit(100).all(db).await?;

            Ok::<_, sea_orm::DbErr>((stats, jobs))
        });

        match result {
            Ok((stats, jobs)) => {
                self.jobs.stats = stats;
                self.jobs.jobs = jobs;
                if !self.jobs.jobs.is_empty() && self.jobs.bottom_state.selected().is_none() {
                    self.jobs.bottom_state.select(Some(0));
                }
                if !self.jobs.stats.is_empty() && self.jobs.top_state.selected().is_none() {
                    self.jobs.top_state.select(Some(0));
                }
            }
            Err(e) => {
                self.message = Some((format!("Failed to load jobs: {e}"), true));
            }
        }
    }

    fn do_retry_job(&mut self, job_id: Uuid) {
        let db = self.db;
        let result = self.handle.block_on(async {
            let active = job::ActiveModel {
                id: Set(job_id),
                status: Set(JobStatus::Pending),
                next_execution_at: Set(None),
                ..Default::default()
            };
            job::Entity::update(active).exec(db).await
        });

        match result {
            Ok(_) => {
                self.message = Some(("Job queued for retry.".to_string(), false));
                self.load_jobs();
            }
            Err(e) => {
                self.message = Some((format!("Failed to retry job: {e}"), true));
            }
        }
    }

    // ── Key handling ──────────────────────────────────────────────────────────

    /// Returns true if the app should quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Clear message on any key except the ones that just triggered it
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
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.load_dashboard();
            }
            _ => {}
        }
        false
    }

    fn handle_key_users(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => {
                self.screen = Screen::Dashboard;
            }
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
                    if let Some(u) = self.users.users.get(idx).cloned() {
                        self.load_user_detail(u);
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
        let Screen::UserDetail { ref user, .. } = self.screen else {
            return false;
        };
        let user_id = user.id;
        let user_clone = user.clone();

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => {
                self.screen = Screen::Users;
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                self.screen = Screen::GiftSubscription {
                    user: user_clone,
                    plan_idx: 0,
                    days_input: "30".to_string(),
                    confirming: false,
                };
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.do_activate_user(user_id);
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                self.screen = Screen::DeleteConfirm {
                    user: user_clone,
                    email_input: String::new(),
                };
            }
            _ => {}
        }
        false
    }

    fn handle_key_gift(&mut self, key: KeyEvent) -> bool {
        let Screen::GiftSubscription {
            ref user,
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
                let u = user.clone();
                self.load_user_detail(u);
                return false;
            }
            KeyCode::Tab => {
                *confirming = !*confirming;
            }
            KeyCode::Up if !*confirming => {
                if *plan_idx > 0 {
                    *plan_idx -= 1;
                }
            }
            KeyCode::Down if !*confirming => {
                if !self.plans.is_empty() && *plan_idx + 1 < self.plans.len() {
                    *plan_idx += 1;
                }
            }
            KeyCode::Backspace if *confirming => {
                days_input.pop();
            }
            KeyCode::Char(c) if *confirming && c.is_ascii_digit() => {
                days_input.push(c);
            }
            KeyCode::Enter => {
                let user_id = user.id;
                let plan = self.plans.get(*plan_idx).cloned().unwrap_or_default();
                let days: i64 = days_input.parse().unwrap_or(30);
                if plan.is_empty() {
                    self.message = Some(("No plan selected.".to_string(), true));
                } else if days <= 0 {
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
            ref user,
            ref mut email_input,
        } = self.screen
        else {
            return false;
        };

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => {
                let u = user.clone();
                self.load_user_detail(u);
                return false;
            }
            KeyCode::Backspace => {
                email_input.pop();
            }
            KeyCode::Char(c) => {
                email_input.push(c);
            }
            KeyCode::Enter => {
                if email_input == &user.email {
                    let user_id = user.id;
                    self.do_delete_user(user_id);
                } else {
                    self.message = Some(("Email does not match. Deletion cancelled.".to_string(), true));
                    let u = user.clone();
                    self.load_user_detail(u);
                }
            }
            _ => {}
        }
        false
    }

    fn handle_key_jobs(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => {
                self.screen = Screen::Dashboard;
            }
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
                    None => Some(JobStatus::Failed),
                    Some(JobStatus::Failed) => Some(JobStatus::Pending),
                    Some(JobStatus::Pending) => Some(JobStatus::Running),
                    Some(JobStatus::Running) => None,
                    _ => None,
                };
                self.load_jobs();
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                // Filter by the type currently selected in top panel
                if self.jobs.panel == JobPanel::Top {
                    if let Some(idx) = self.jobs.top_state.selected() {
                        if let Some((t, ..)) = self.jobs.stats.get(idx) {
                            let t = t.clone();
                            self.jobs.type_filter = if self.jobs.type_filter.as_deref() == Some(&t) {
                                None
                            } else {
                                Some(t)
                            };
                            self.load_jobs();
                        }
                    }
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.jobs.panel == JobPanel::Bottom {
                    if let Some(idx) = self.jobs.bottom_state.selected() {
                        if let Some(j) = self.jobs.jobs.get(idx) {
                            if j.status == JobStatus::Failed {
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

    // ── Rendering ─────────────────────────────────────────────────────────────

    fn render(&mut self, f: &mut Frame) {
        let area = f.area();

        // Global status bar at the bottom
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);

        let status_text = if let Some((msg, is_err)) = &self.message {
            let style = if *is_err {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };
            Line::from(Span::styled(msg.clone(), style))
        } else {
            Line::from(Span::raw(""))
        };
        f.render_widget(Paragraph::new(status_text), chunks[1]);

        match &self.screen {
            Screen::Dashboard => self.render_dashboard(f, chunks[0]),
            Screen::Users => self.render_users(f, chunks[0]),
            Screen::UserDetail { .. } => self.render_user_detail(f, chunks[0]),
            Screen::GiftSubscription { .. } => self.render_gift(f, chunks[0]),
            Screen::DeleteConfirm { .. } => self.render_delete_confirm(f, chunks[0]),
            Screen::Jobs => self.render_jobs(f, chunks[0]),
        }
    }

    fn render_dashboard(&self, f: &mut Frame, area: Rect) {
        let d = &self.dashboard;

        let block = Block::default()
            .title(" Erno Admin — Dashboard ")
            .borders(Borders::ALL);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(7),
                Constraint::Length(1),
                Constraint::Length(5),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(inner);

        // Users section
        let user_block = Block::default().title(" Users ").borders(Borders::ALL);
        let user_text = if d.loaded {
            vec![
                Line::from(format!("  Total users:   {}", d.total_users)),
                Line::from(format!("  Stripe active: {}", d.stripe_active)),
                Line::from(format!("  Gift active:   {}", d.gift_active)),
                Line::from(format!("  Trial active:  {}", d.trial_active)),
                Line::from(format!("  No sub:        {}", d.no_sub)),
            ]
        } else {
            vec![Line::from("  Loading...")]
        };
        f.render_widget(
            Paragraph::new(user_text).block(user_block),
            rows[1],
        );

        // Jobs section
        let job_block = Block::default().title(" Job Queue ").borders(Borders::ALL);
        let job_text = if d.loaded {
            vec![
                Line::from(format!("  Pending:  {}", d.pending_jobs)),
                Line::from(format!("  Running:  {}", d.running_jobs)),
                Line::from(format!("  Failed:   {}", d.failed_jobs)),
            ]
        } else {
            vec![Line::from("  Loading...")]
        };
        f.render_widget(
            Paragraph::new(job_text).block(job_block),
            rows[3],
        );

        // Help
        let help = Paragraph::new(vec![
            Line::from(Span::styled(
                "  [u] Users   [j] Jobs   [r] Refresh   [q] Quit",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
        f.render_widget(help, rows[5]);
    }

    fn render_users(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(2)])
            .split(area);

        // Search bar
        let search = Paragraph::new(self.users.query.clone())
            .block(
                Block::default()
                    .title(" Search by email — type to filter ")
                    .borders(Borders::ALL),
            );
        f.render_widget(search, chunks[0]);

        // User table
        let header = Row::new(vec!["Email", "Verified", "Subscription", "Plan"])
            .style(Style::default().add_modifier(Modifier::BOLD))
            .height(1);

        let rows: Vec<Row> = self
            .users
            .users
            .iter()
            .map(|u| {
                Row::new(vec![
                    Cell::from(u.email.clone()),
                    Cell::from(if u.email_verified_at.is_some() { "✓" } else { "✗" }),
                    Cell::from(u.subscription_type.as_deref().unwrap_or("-")),
                    Cell::from(u.subscription_plan.as_deref().unwrap_or("-")),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Min(30),
                Constraint::Length(8),
                Constraint::Length(10),
                Constraint::Length(15),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .title(format!(" Users ({}) ", self.users.users.len()))
                .borders(Borders::ALL),
        )
        .row_highlight_style(Style::default().bg(Color::Blue).fg(Color::White));

        f.render_stateful_widget(table, chunks[1], &mut self.users.table_state);

        let help = Paragraph::new(Line::from(Span::styled(
            "  ↑↓ Navigate   Enter Select   Esc Back   Type to search",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[2]);
    }

    fn render_user_detail(&self, f: &mut Frame, area: Rect) {
        let Screen::UserDetail { user: u, subscription } = &self.screen else {
            return;
        };

        let block = Block::default()
            .title(format!(" User: {} ", u.email))
            .borders(Borders::ALL);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Length(9),
                Constraint::Min(0),
                Constraint::Length(2),
            ])
            .split(inner);

        // User info
        let verified = u
            .email_verified_at
            .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "Not verified".to_string());
        let user_text = vec![
            Line::from(format!("  ID:       {}", u.id)),
            Line::from(format!("  Email:    {}", u.email)),
            Line::from(format!("  Verified: {verified}")),
            Line::from(format!("  Created:  {}", u.created_at.format("%Y-%m-%d"))),
        ];
        f.render_widget(
            Paragraph::new(user_text).block(Block::default().title(" Info ").borders(Borders::ALL)),
            chunks[0],
        );

        // Subscription info
        let sub_text = if let Some(s) = subscription {
            let mut lines = vec![
                Line::from(format!("  Type:   {}", s.sub_type)),
                Line::from(format!("  Plan:   {}", s.plan)),
                Line::from(format!("  Status: {}", s.status)),
                Line::from(format!("  Expiry: {}", s.expiry)),
            ];
            if let Some(cid) = &s.stripe_customer_id {
                lines.push(Line::from(format!("  Stripe customer: {cid}")));
            }
            if let Some(sid) = &s.stripe_sub_id {
                lines.push(Line::from(format!("  Stripe sub:      {sid}")));
            }
            if let Some(cancel) = s.cancel_at_period_end {
                lines.push(Line::from(format!(
                    "  Cancel at period end: {}",
                    if cancel { "yes" } else { "no" }
                )));
            }
            lines
        } else {
            vec![Line::from("  No active subscription.")]
        };
        f.render_widget(
            Paragraph::new(sub_text)
                .block(Block::default().title(" Subscription ").borders(Borders::ALL)),
            chunks[1],
        );

        let verify_action = if u.email_verified_at.is_none() {
            "[a] Activate"
        } else {
            ""
        };
        let help = Paragraph::new(Line::from(Span::styled(
            format!("  [g] Gift sub   {verify_action}   [x] Delete user   Esc Back"),
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[3]);
    }

    fn render_gift(&self, f: &mut Frame, area: Rect) {
        let Screen::GiftSubscription {
            user: u,
            plan_idx,
            days_input,
            confirming,
        } = &self.screen
        else {
            return;
        };

        let block = Block::default()
            .title(format!(" Gift Subscription — {} ", u.email))
            .borders(Borders::ALL);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(2),
            ])
            .split(inner);

        // Plan selector
        let plan_items: Vec<ListItem> = self
            .plans
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if i == *plan_idx {
                    ListItem::new(format!("  ► {p}"))
                        .style(Style::default().bg(Color::Blue).fg(Color::White))
                } else {
                    ListItem::new(format!("    {p}"))
                }
            })
            .collect();

        let plan_block_title = if !confirming {
            " Plan (↑↓ to select, Tab→ days) "
        } else {
            " Plan "
        };
        let plan_list =
            List::new(plan_items).block(Block::default().title(plan_block_title).borders(Borders::ALL));
        f.render_widget(plan_list, chunks[0]);

        // Days input
        let days_block_title = if *confirming {
            " Duration in days (type digits, Enter to confirm) "
        } else {
            " Duration in days (Tab to edit) "
        };
        let days_widget = Paragraph::new(days_input.clone())
            .block(Block::default().title(days_block_title).borders(Borders::ALL));
        f.render_widget(days_widget, chunks[1]);

        let help = Paragraph::new(Line::from(Span::styled(
            "  Tab Focus field   ↑↓ Plan   Enter Confirm   Esc Cancel",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[2]);
    }

    fn render_delete_confirm(&self, f: &mut Frame, area: Rect) {
        let Screen::DeleteConfirm { user: u, email_input } = &self.screen else {
            return;
        };

        let block = Block::default()
            .title(" Confirm User Deletion ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(2),
            ])
            .split(inner);

        let warning = Paragraph::new(vec![
            Line::from(Span::styled(
                "  ⚠  This will permanently delete the user and all their data.",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("  User: {}", u.email)),
            Line::from(""),
            Line::from("  Type the user's email to confirm:"),
        ]);
        f.render_widget(warning, chunks[0]);

        let input = Paragraph::new(email_input.clone())
            .block(Block::default().title(" Email ").borders(Borders::ALL));
        f.render_widget(input, chunks[1]);

        let help = Paragraph::new(Line::from(Span::styled(
            "  Enter Confirm   Esc Cancel",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[3]);
    }

    fn render_jobs(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(35), Constraint::Min(0), Constraint::Length(2)])
            .split(area);

        // Top: stats per job type
        let stat_header = Row::new(vec!["Type", "Pending", "Running", "Failed", "Completed"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let stat_rows: Vec<Row> = self
            .jobs
            .stats
            .iter()
            .map(|(t, p, r, f, c)| {
                Row::new(vec![
                    Cell::from(t.clone()),
                    Cell::from(p.to_string()),
                    Cell::from(r.to_string()),
                    Cell::from(if *f > 0 {
                        Span::styled(f.to_string(), Style::default().fg(Color::Red))
                    } else {
                        Span::raw(f.to_string())
                    }),
                    Cell::from(c.to_string()),
                ])
            })
            .collect();

        let top_title = if self.jobs.panel == JobPanel::Top {
            " Job Stats (active) — Tab to switch, T to filter by type "
        } else {
            " Job Stats — Tab to switch "
        };

        let stat_table = Table::new(
            stat_rows,
            [
                Constraint::Min(30),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Length(11),
            ],
        )
        .header(stat_header)
        .block(Block::default().title(top_title).borders(Borders::ALL))
        .row_highlight_style(Style::default().bg(Color::Blue).fg(Color::White));

        f.render_stateful_widget(stat_table, chunks[0], &mut self.jobs.top_state);

        // Bottom: job list
        let filter_label = match &self.jobs.status_filter {
            Some(s) => format!("{s}"),
            None => "All".to_string(),
        };
        let type_label = self
            .jobs
            .type_filter
            .as_deref()
            .unwrap_or("all types");

        let job_header = Row::new(vec!["Type", "Status", "Retries", "Next run", "Created"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let job_rows: Vec<Row> = self
            .jobs
            .jobs
            .iter()
            .map(|j| {
                let status_cell = Cell::from(Span::styled(
                    format!("{}", j.status),
                    match j.status {
                        JobStatus::Failed => Style::default().fg(Color::Red),
                        JobStatus::Running => Style::default().fg(Color::Yellow),
                        JobStatus::Completed => Style::default().fg(Color::Green),
                        _ => Style::default(),
                    },
                ));
                let next = j
                    .next_execution_at
                    .map(|d| d.format("%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".to_string());
                Row::new(vec![
                    Cell::from(j.r#type.clone()),
                    status_cell,
                    Cell::from(j.retry_count.to_string()),
                    Cell::from(next),
                    Cell::from(j.created_at.format("%m-%d %H:%M").to_string()),
                ])
            })
            .collect();

        let bottom_title = if self.jobs.panel == JobPanel::Bottom {
            format!(
                " Jobs ({}) status={filter_label} type={type_label} — R retry, F filter ",
                self.jobs.jobs.len()
            )
        } else {
            format!(
                " Jobs ({}) status={filter_label} type={type_label} ",
                self.jobs.jobs.len()
            )
        };

        let job_table = Table::new(
            job_rows,
            [
                Constraint::Min(30),
                Constraint::Length(13),
                Constraint::Length(8),
                Constraint::Length(12),
                Constraint::Length(12),
            ],
        )
        .header(job_header)
        .block(Block::default().title(bottom_title).borders(Borders::ALL))
        .row_highlight_style(Style::default().bg(Color::Blue).fg(Color::White));

        f.render_stateful_widget(job_table, chunks[1], &mut self.jobs.bottom_state);

        let help = Paragraph::new(Line::from(Span::styled(
            "  Tab Panel   ↑↓ Navigate   [r] Retry/Refresh   [f] Filter status   [t] Filter type   Esc Back",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[2]);
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn run(
    db: &DatabaseConnection,
    plans: &[String],
    handle: &Handle,
) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_inner(&mut terminal, db, plans, handle);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_inner(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    db: &DatabaseConnection,
    plans: &[String],
    handle: &Handle,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = AdminApp::new(db, plans, handle);
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
