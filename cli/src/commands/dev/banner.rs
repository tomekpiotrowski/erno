use std::time::Duration;

use reqwest::Client;

use crate::ui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceState {
    Starting,
    Migrating,
    Ready,
}

impl ServiceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Migrating => "migrating",
            Self::Ready => "ready",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Starting => ui::icon::STARTING,
            Self::Migrating => ui::icon::MIGRATING,
            Self::Ready => ui::icon::READY,
        }
    }

    fn style(self) -> anstyle::Style {
        match self {
            Self::Starting | Self::Migrating => ui::YELLOW,
            Self::Ready => ui::GREEN,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DevUrls {
    pub api: Option<String>,
    pub app: Option<String>,
    pub www: Option<String>,
    pub prometheus: Option<String>,
    pub tempo: Option<String>,
    pub loki: Option<String>,
    /// The monitoring collector. An Erno app of its own, so it gets the same
    /// readiness/liveness treatment as `api` — it migrates its database on boot
    /// and the banner should say so rather than sitting on "starting".
    pub monitoring: Option<String>,
    /// The monitoring operator console.
    pub console: Option<String>,
    pub admin: Option<String>,
    /// Extra `[[package.dev]]` services: (name, url), declaration order.
    pub extra: Vec<(String, String)>,
}

impl DevUrls {
    #[cfg(test)]
    pub fn defaults(start_api: bool, start_app: bool, start_www: bool) -> Self {
        Self {
            api: start_api.then(|| "http://localhost:3000".to_string()),
            app: start_app.then(|| "http://localhost:4200".to_string()),
            www: start_www.then(|| "http://localhost:4321".to_string()),
            prometheus: start_api.then(|| super::prometheus::LISTEN_URL.to_string()),
            tempo: start_api.then(|| super::tempo::LISTEN_URL.to_string()),
            loki: start_api.then(|| super::loki::LISTEN_URL.to_string()),
            monitoring: start_api.then(super::monitoring_url),
            console: start_api.then(|| super::CONSOLE_URL.to_string()),
            admin: start_api.then(|| super::ADMIN_URL.to_string()),
            extra: Vec::new(),
        }
    }

    fn extra_names(&self) -> impl Iterator<Item = &str> {
        self.extra.iter().map(|(name, _)| name.as_str())
    }

    pub fn api_readiness(&self) -> Option<String> {
        self.api
            .as_deref()
            .map(|u| format!("{}/readiness", u.trim_end_matches('/')))
    }

    pub fn api_liveness(&self) -> Option<String> {
        self.api
            .as_deref()
            .map(|u| format!("{}/liveness", u.trim_end_matches('/')))
    }

    pub fn monitoring_readiness(&self) -> Option<String> {
        self.monitoring
            .as_deref()
            .map(|u| format!("{}/readiness", u.trim_end_matches('/')))
    }

    pub fn monitoring_liveness(&self) -> Option<String> {
        self.monitoring
            .as_deref()
            .map(|u| format!("{}/liveness", u.trim_end_matches('/')))
    }

    pub fn prometheus_ready(&self) -> Option<String> {
        self.prometheus
            .as_deref()
            .map(|u| format!("{}/-/ready", u.trim_end_matches('/')))
    }

    pub fn tempo_ready(&self) -> Option<String> {
        self.tempo
            .as_deref()
            .map(|u| format!("{}/ready", u.trim_end_matches('/')))
    }

    pub fn loki_ready(&self) -> Option<String> {
        self.loki
            .as_deref()
            .map(|u| format!("{}/ready", u.trim_end_matches('/')))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BannerSnapshot {
    pub api: Option<ServiceState>,
    pub app: Option<ServiceState>,
    pub www: Option<ServiceState>,
    pub prometheus: Option<ServiceState>,
    pub tempo: Option<ServiceState>,
    pub loki: Option<ServiceState>,
    pub monitoring: Option<ServiceState>,
    pub console: Option<ServiceState>,
    pub admin: Option<ServiceState>,
    pub extra: Vec<ServiceState>,
}

/// One row of the banner: a service, its URL, its readiness, and anything else
/// worth knowing about it.
struct BannerRow {
    name: String,
    url: String,
    state: ServiceState,
    note: Option<&'static str>,
}

/// The banner's rows, in banner order.
///
/// Every row is a service that was actually started, so every row has a state.
/// The `/dev/emails` and `/dev/jobs` links used to sit here too, as stateless
/// rows below a blank line; they were three rows of the pinned region spent on
/// two URLs nobody looked up from the banner.
fn banner_rows(urls: &DevUrls, snap: &BannerSnapshot) -> Vec<BannerRow> {
    let mut rows = Vec::new();
    let mut push = |name: &str, url: &str, state, note| {
        rows.push(BannerRow {
            name: name.to_string(),
            url: url.to_string(),
            state,
            note,
        })
    };
    if let (Some(url), Some(state)) = (urls.www.as_deref(), snap.www) {
        push("www", url, state, None);
    }
    if let (Some(url), Some(state)) = (urls.app.as_deref(), snap.app) {
        push("app", url, state, None);
    }
    if let (Some(url), Some(state)) = (urls.api.as_deref(), snap.api) {
        push("api", url, state, None);
    }
    if let (Some(url), Some(state)) = (urls.prometheus.as_deref(), snap.prometheus) {
        push("prom", url, state, None);
    }
    if let (Some(url), Some(state)) = (urls.tempo.as_deref(), snap.tempo) {
        push("tempo", url, state, None);
    }
    if let (Some(url), Some(state)) = (urls.loki.as_deref(), snap.loki) {
        push("loki", url, state, None);
    }
    if let (Some(url), Some(state)) = (urls.monitoring.as_deref(), snap.monitoring) {
        push("mon", url, state, None);
    }
    if let (Some(url), Some(state)) = (urls.console.as_deref(), snap.console) {
        push("console", url, state, Some("password: admin"));
    }
    if let (Some(url), Some(state)) = (urls.admin.as_deref(), snap.admin) {
        push("admin", url, state, Some("password: admin"));
    }
    for ((name, url), state) in urls.extra.iter().zip(&snap.extra) {
        push(name, url, *state, None);
    }
    rows
}

/// Started services in banner order: `(name, url)`.
pub fn listed_services(urls: &DevUrls) -> Vec<(String, String)> {
    banner_rows(urls, &starting_snapshot(urls))
        .into_iter()
        .map(|r| (r.name, r.url))
        .collect()
}

pub fn render_banner(urls: &DevUrls, snap: &BannerSnapshot) -> String {
    render_banner_when(ui::Face::current(), urls, snap)
}

pub fn render_banner_when(face: ui::Face, urls: &DevUrls, snap: &BannerSnapshot) -> String {
    let rows = banner_rows(urls, snap);

    // Every column is sized from the content, across every row, so they line up
    // no matter how long the URLs are. `ui::column_width` measures screen
    // columns, so the icons do not skew it.
    let name_w = ui::column_width(rows.iter().map(|r| r.name.as_str()));
    let url_w = ui::column_width(rows.iter().map(|r| r.url.as_str()));
    let state_w = ui::column_width(rows.iter().map(|r| r.state.label()));

    let row = |r: &BannerRow| {
        let lead = if face.emoji {
            format!("{} ", ui::label_icon(&r.name))
        } else {
            String::new()
        };
        let name = ui::paint_when(
            face.color,
            ui::label_style(&r.name),
            &format!("{:<name_w$}", r.name),
        );
        let state = ui::paint_when(face.color, r.state.style(), r.state.label());
        let state = if face.emoji {
            format!("{} {state}", r.state.icon())
        } else {
            state
        };
        // The state column is only padded when something follows it. Padding
        // inside the paint would put the spaces before the reset, where
        // `trim_end` cannot see them, and every row would end in whitespace.
        let tail = match r.note {
            Some(note) => format!(
                "{state}{}  {}",
                " ".repeat(state_w - ui::display_width(r.state.label())),
                ui::paint_when(face.color, ui::DIM, note),
            ),
            None => state,
        };
        format!("  {lead}{name}  {:<url_w$}  {tail}\n", r.url)
            .trim_end()
            .to_string()
            + "\n"
    };

    let mut out = String::from("\n");
    let lead = if face.emoji {
        format!("{} ", ui::icon::DEV)
    } else {
        String::new()
    };
    out.push_str(&format!(
        "  {lead}{}\n",
        ui::paint_when(face.color, ui::DIM, "erno — Ctrl+C to stop")
    ));
    for r in &rows {
        out.push_str(&row(r));
    }
    out.push('\n');
    out
}

pub fn print_banner(urls: &DevUrls, snap: &BannerSnapshot) {
    // Narration, like everything else the CLI says about itself — and one
    // block, so a child's line cannot land in the middle of it.
    ui::emit_block(ui::Stream::Err, &render_banner(urls, snap));
}

/// The banner's rows, ready to pin: [`render_banner_when`] without the trailing
/// blank line. The leading one is kept, separating the region from the log
/// lines scrolling above it.
///
/// The row count does not depend on the snapshot, so the pinned region never
/// changes height as services come up.
pub fn region_lines(face: ui::Face, urls: &DevUrls, snap: &BannerSnapshot) -> Vec<String> {
    let text = render_banner_when(face, urls, snap);
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines
}

/// Show the banner: pinned to the bottom of the terminal where that works,
/// scrolled otherwise.
///
/// `Some` means the region is live, and the caller must not also print
/// transition rows — the region already shows every state, and doing both would
/// bring back exactly the duplication this replaced. Hold the guard for the
/// session: dropping it takes the region down.
pub fn start(urls: &DevUrls) -> Option<ui::Pinned> {
    let snap = starting_snapshot(urls);
    let pinned = ui::pin(&region_lines(ui::Face::current(), urls, &snap));
    if pinned.is_none() {
        print_banner(urls, &snap);
    }
    pinned
}

/// One service's state change, as the readiness watcher observes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    name: String,
    state: ServiceState,
}

/// The banner's built-in services, in banner order.
const SERVICES: [&str; 9] = [
    "www", "app", "api", "prom", "tempo", "loki", "mon", "console", "admin",
];

fn transitions(urls: &DevUrls, prev: &BannerSnapshot, next: &BannerSnapshot) -> Vec<Transition> {
    let pairs = [
        (prev.www, next.www),
        (prev.app, next.app),
        (prev.api, next.api),
        (prev.prometheus, next.prometheus),
        (prev.tempo, next.tempo),
        (prev.loki, next.loki),
        (prev.monitoring, next.monitoring),
        (prev.console, next.console),
        (prev.admin, next.admin),
    ];
    let mut out: Vec<Transition> = SERVICES
        .iter()
        .zip(pairs)
        .filter_map(|(name, (before, after))| match (before, after) {
            (Some(before), Some(after)) if before != after => Some(Transition {
                name: (*name).to_string(),
                state: after,
            }),
            _ => None,
        })
        .collect();
    for ((name, _), (before, after)) in urls
        .extra
        .iter()
        .zip(prev.extra.iter().zip(next.extra.iter()))
    {
        if before != after {
            out.push(Transition {
                name: name.clone(),
                state: *after,
            });
        }
    }
    out
}

/// The banner is printed once; every later change is a single row instead of
/// another full copy. The name column is sized from every service name, not
/// just the ones in this batch, so rows printed seconds apart still line up.
/// A state change as a row. The row's own marker carries the state, so unlike
/// the banner there is no second state icon here — that would say it twice.
pub fn render_transitions(face: ui::Face, changes: &[Transition], extra_names: &[&str]) -> String {
    let name_w = ui::column_width(SERVICES.iter().copied().chain(extra_names.iter().copied()));
    let mut out = String::new();
    for change in changes {
        let level = match change.state {
            ServiceState::Ready => ui::Level::Ok,
            ServiceState::Starting | ServiceState::Migrating => ui::Level::Info,
        };
        let lead = if face.emoji {
            format!("{} ", ui::label_icon(&change.name))
        } else {
            String::new()
        };
        let name = ui::paint_when(
            face.color,
            ui::label_style(&change.name),
            &format!("{:<name_w$}", change.name),
        );
        let state = ui::paint_when(face.color, change.state.style(), change.state.label());
        out.push_str(&ui::render_row(
            face,
            level,
            &format!("{lead}{name}  {state}"),
        ));
        out.push('\n');
    }
    out
}

fn print_transitions(changes: &[Transition], extra_names: &[&str]) {
    // These are `ok`/`info` rows, so `--quiet` drops them like any other.
    if ui::quiet() {
        return;
    }
    ui::emit_block(
        ui::Stream::Err,
        &render_transitions(ui::Face::current(), changes, extra_names),
    );
}

pub fn starting_snapshot(urls: &DevUrls) -> BannerSnapshot {
    BannerSnapshot {
        api: urls.api.as_ref().map(|_| ServiceState::Starting),
        app: urls.app.as_ref().map(|_| ServiceState::Starting),
        www: urls.www.as_ref().map(|_| ServiceState::Starting),
        prometheus: urls.prometheus.as_ref().map(|_| ServiceState::Starting),
        tempo: urls.tempo.as_ref().map(|_| ServiceState::Starting),
        loki: urls.loki.as_ref().map(|_| ServiceState::Starting),
        monitoring: urls.monitoring.as_ref().map(|_| ServiceState::Starting),
        console: urls.console.as_ref().map(|_| ServiceState::Starting),
        admin: urls.admin.as_ref().map(|_| ServiceState::Starting),
        extra: urls.extra.iter().map(|_| ServiceState::Starting).collect(),
    }
}

/// Probe HTTP endpoints and report whenever a service changes state: by
/// redrawing the pinned region when there is one, and by printing a single row
/// per change when the banner scrolled instead.
pub fn spawn_readiness_watcher(urls: DevUrls, sticky: bool) {
    tokio::spawn(async move {
        let client = match Client::builder()
            .timeout(Duration::from_millis(500))
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        let mut last = starting_snapshot(&urls);
        loop {
            let next = probe_all(&client, &urls).await;
            if next != last {
                if sticky {
                    ui::repin(&region_lines(ui::Face::current(), &urls, &next));
                } else {
                    let names: Vec<&str> = urls.extra_names().collect();
                    print_transitions(&transitions(&urls, &last, &next), &names);
                }
                last = next;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });
}

pub fn state_named(snap: &BannerSnapshot, urls: &DevUrls, name: &str) -> Option<ServiceState> {
    match name {
        "www" => snap.www,
        "app" => snap.app,
        "api" => snap.api,
        "prom" => snap.prometheus,
        "tempo" => snap.tempo,
        "loki" => snap.loki,
        "mon" => snap.monitoring,
        "console" => snap.console,
        "admin" => snap.admin,
        other => urls
            .extra
            .iter()
            .zip(&snap.extra)
            .find(|((n, _), _)| n == other)
            .map(|(_, s)| *s),
    }
}

pub async fn probe_all(client: &Client, urls: &DevUrls) -> BannerSnapshot {
    let prom_ready = urls.prometheus_ready();
    let tempo_ready = urls.tempo_ready();
    let loki_ready = urls.loki_ready();
    let (api, app, www, prometheus, tempo, loki, monitoring, console, admin, extra) = tokio::join!(
        async {
            match urls.api.as_deref() {
                Some(_) => {
                    Some(probe_erno(client, urls.api_readiness(), urls.api_liveness()).await)
                }
                None => None,
            }
        },
        async {
            match urls.app.as_deref() {
                Some(url) => Some(probe_http(client, url).await),
                None => None,
            }
        },
        async {
            match urls.www.as_deref() {
                Some(url) => Some(probe_http(client, url).await),
                None => None,
            }
        },
        async {
            match &prom_ready {
                Some(url) => Some(probe_http(client, url).await),
                None => None,
            }
        },
        async {
            match &tempo_ready {
                Some(url) => Some(probe_http(client, url).await),
                None => None,
            }
        },
        async {
            match &loki_ready {
                Some(url) => Some(probe_http(client, url).await),
                None => None,
            }
        },
        async {
            match urls.monitoring.as_deref() {
                Some(_) => Some(
                    probe_erno(
                        client,
                        urls.monitoring_readiness(),
                        urls.monitoring_liveness(),
                    )
                    .await,
                ),
                None => None,
            }
        },
        async {
            match urls.console.as_deref() {
                Some(url) => Some(probe_http(client, url).await),
                None => None,
            }
        },
        async {
            match urls.admin.as_deref() {
                Some(url) => Some(probe_http(client, url).await),
                None => None,
            }
        },
        async {
            let mut extra = Vec::with_capacity(urls.extra.len());
            for (_, url) in &urls.extra {
                extra.push(probe_http(client, url).await);
            }
            extra
        },
    );
    BannerSnapshot {
        api,
        app,
        www,
        prometheus,
        tempo,
        loki,
        monitoring,
        console,
        admin,
        extra,
    }
}

/// Probe an Erno service, which serves both `/readiness` and `/liveness`.
///
/// The two endpoints together distinguish "not up yet" from "up but still
/// migrating", which is why this is not just [`probe_http`]. Both the api and
/// the monitoring collector are Erno apps, so both go through here.
async fn probe_erno(
    client: &Client,
    readiness: Option<String>,
    liveness: Option<String>,
) -> ServiceState {
    let Some(readiness) = readiness else {
        return ServiceState::Starting;
    };
    let (ready, live) = tokio::join!(is_up(client, &readiness), async {
        match &liveness {
            Some(url) => is_up(client, url).await,
            None => false,
        }
    });
    if ready {
        ServiceState::Ready
    } else if live {
        ServiceState::Migrating
    } else {
        ServiceState::Starting
    }
}

async fn probe_http(client: &Client, url: &str) -> ServiceState {
    if is_up(client, url).await {
        ServiceState::Ready
    } else {
        ServiceState::Starting
    }
}

async fn is_up(client: &Client, url: &str) -> bool {
    match client.get(url).send().await {
        Ok(res) => res.status().as_u16() < 500,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full treatment: colour and icons.
    const FANCY: ui::Face = ui::Face {
        color: true,
        emoji: true,
    };

    fn snapshot(state: ServiceState, urls: &DevUrls) -> BannerSnapshot {
        BannerSnapshot {
            api: urls.api.as_ref().map(|_| state),
            app: urls.app.as_ref().map(|_| state),
            www: urls.www.as_ref().map(|_| state),
            prometheus: urls.prometheus.as_ref().map(|_| state),
            tempo: urls.tempo.as_ref().map(|_| state),
            loki: urls.loki.as_ref().map(|_| state),
            monitoring: urls.monitoring.as_ref().map(|_| state),
            console: urls.console.as_ref().map(|_| state),
            admin: urls.admin.as_ref().map(|_| state),
            extra: urls.extra.iter().map(|_| state).collect(),
        }
    }

    #[test]
    fn banner_includes_urls_and_states() {
        let urls = DevUrls::defaults(true, true, true);
        let snap = BannerSnapshot {
            api: Some(ServiceState::Ready),
            app: Some(ServiceState::Starting),
            www: Some(ServiceState::Ready),
            prometheus: Some(ServiceState::Ready),
            tempo: Some(ServiceState::Ready),
            loki: Some(ServiceState::Ready),
            monitoring: Some(ServiceState::Ready),
            console: Some(ServiceState::Ready),
            admin: Some(ServiceState::Ready),
            extra: vec![],
        };
        let text = render_banner_when(ui::Face::PLAIN, &urls, &snap);
        assert!(text.contains("http://localhost:3000"));
        assert!(text.contains("http://localhost:4200"));
        assert!(text.contains("http://localhost:4321"));
        assert!(text.contains("http://localhost:9090"));
        assert!(text.contains("http://localhost:4300"));
        assert!(text.contains("ready"));
        assert!(text.contains("starting"));
    }

    #[test]
    fn the_banner_is_services_only() {
        // `mail` and `jobs` were three rows of a pinned region spent on two
        // URLs that never changed. Every row here is a service with a state.
        let urls = DevUrls::defaults(true, true, true);
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        assert!(!text.contains("/dev/emails"));
        assert!(!text.contains("/dev/jobs"));
        // And no blank line wedged into the middle of the list.
        let body: Vec<&str> = text.trim_matches('\n').lines().collect();
        assert!(
            body.iter().all(|l| !l.trim().is_empty()),
            "the banner has a gap in it:\n{text}"
        );
    }

    #[test]
    fn no_banner_row_ends_in_whitespace() {
        // Trailing spaces would be invisible here and load-bearing there: they
        // count against the pinned region's width budget on every redraw.
        let urls = DevUrls::defaults(true, true, true);
        let snap = snapshot(ServiceState::Ready, &urls);
        for face in [ui::Face::PLAIN, FANCY] {
            for line in render_banner_when(face, &urls, &snap).lines() {
                assert_eq!(line, line.trim_end(), "trailing whitespace: {line:?}");
            }
        }
    }

    #[test]
    fn banner_shows_migrating() {
        let urls = DevUrls::defaults(true, true, false);
        let snap = BannerSnapshot {
            api: Some(ServiceState::Migrating),
            app: Some(ServiceState::Starting),
            www: None,
            prometheus: Some(ServiceState::Starting),
            tempo: Some(ServiceState::Starting),
            loki: Some(ServiceState::Starting),
            monitoring: Some(ServiceState::Migrating),
            console: None,
            admin: None,
            extra: vec![],
        };
        let text = render_banner_when(ui::Face::PLAIN, &urls, &snap);
        assert!(text.contains("migrating"));
        assert!(!text.contains("www"));
    }

    #[test]
    fn banner_omits_www_when_absent() {
        let urls = DevUrls::defaults(true, true, false);
        let snap = starting_snapshot(&urls);
        let text = render_banner_when(ui::Face::PLAIN, &urls, &snap);
        assert!(!text.contains("www"));
        assert!(text.contains("api"));
        assert!(urls.www.is_none());
    }

    #[test]
    fn admin_is_a_service_row_with_a_state_and_its_password() {
        let urls = DevUrls::defaults(true, false, false);
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        let row = text
            .lines()
            .find(|l| l.contains(super::super::ADMIN_URL))
            .expect("an admin row");
        assert!(row.contains("starting"));
        assert!(row.contains("password: admin"));

        // And it flips like any other service.
        let ready = render_banner_when(
            ui::Face::PLAIN,
            &urls,
            &snapshot(ServiceState::Ready, &urls),
        );
        assert!(ready
            .lines()
            .any(|l| l.contains(super::super::ADMIN_URL) && l.contains("ready")));
    }

    #[test]
    fn a_service_without_a_url_has_no_row() {
        let mut urls = DevUrls::defaults(true, true, true);
        urls.admin = None;
        // The monitoring console's note also reads "password: admin", so this
        // has to drop both to say anything about the word.
        urls.console = None;
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        assert!(!text.contains(super::super::ADMIN_URL));
        assert!(!text.contains("admin"));
        assert!(!text.contains("password"));
    }

    #[test]
    fn banner_has_no_escapes_when_colour_is_off() {
        let urls = DevUrls::defaults(true, true, true);
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        assert!(!text.contains('\u{1b}'));
    }

    #[test]
    fn banner_columns_line_up() {
        let mut urls = DevUrls::defaults(true, true, true);
        // A long URL must push every row's state column, not just its own.
        urls.www = Some("http://localhost:4321/a/much/longer/path".to_string());
        let snap = BannerSnapshot {
            api: Some(ServiceState::Ready),
            app: Some(ServiceState::Starting),
            www: Some(ServiceState::Ready),
            prometheus: Some(ServiceState::Ready),
            tempo: Some(ServiceState::Ready),
            loki: Some(ServiceState::Ready),
            monitoring: Some(ServiceState::Ready),
            console: Some(ServiceState::Ready),
            admin: Some(ServiceState::Migrating),
            extra: vec![],
        };
        for face in [ui::Face::PLAIN, FANCY] {
            let text = render_banner_when(face, &urls, &snap);
            // Measured in screen columns, because under FANCY the rows carry
            // two-column icons that a byte or `char` offset would misjudge.
            let offsets: Vec<usize> = text
                .lines()
                .filter_map(|l| {
                    ["ready", "starting", "migrating"]
                        .iter()
                        .find_map(|w| l.find(w))
                        .map(|byte| ui::display_width(&l[..byte]))
                })
                .collect();
            assert_eq!(offsets.len(), 9, "expected a state on every row:\n{text}");
            assert!(
                offsets.windows(2).all(|w| w[0] == w[1]),
                "state column ragged: {offsets:?}\n{text}"
            );
        }
    }

    #[test]
    fn banner_omits_unselected_services() {
        let urls = DevUrls::defaults(true, false, false);
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        assert!(text.contains("api"));
        assert!(text.contains("prom"));
        assert!(text.contains("http://localhost:9090"));
        assert!(!text.contains("app"));
        assert!(!text.contains("www"));
    }

    #[test]
    fn region_lines_are_the_banner_without_the_trailing_blank() {
        let urls = DevUrls::defaults(true, true, true);
        let snap = starting_snapshot(&urls);
        let lines = region_lines(ui::Face::PLAIN, &urls, &snap);
        let text = render_banner_when(ui::Face::PLAIN, &urls, &snap);

        assert_eq!(lines.first().map(String::as_str), Some(""));
        assert!(!lines.last().unwrap().trim().is_empty());
        assert!(lines.iter().all(|l| !l.contains('\n')));
        assert!(lines.iter().all(|l| !l.contains('\u{1b}')));
        // Same content, same order — the layout maths is not duplicated.
        assert_eq!(lines.join("\n"), text.trim_end_matches('\n'));
    }

    #[test]
    fn the_pinned_region_never_changes_height() {
        // A region that grew or shrank between redraws would leave the
        // cursor-up count wrong and eat a row of log output.
        let urls = DevUrls::defaults(true, true, true);
        let starting = region_lines(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        let ready = region_lines(
            ui::Face::PLAIN,
            &urls,
            &BannerSnapshot {
                api: Some(ServiceState::Ready),
                app: Some(ServiceState::Ready),
                www: Some(ServiceState::Migrating),
                prometheus: Some(ServiceState::Ready),
                tempo: Some(ServiceState::Ready),
                loki: Some(ServiceState::Ready),
                monitoring: Some(ServiceState::Ready),
                console: Some(ServiceState::Ready),
                admin: Some(ServiceState::Ready),
                extra: vec![],
            },
        );
        assert_eq!(starting.len(), ready.len());
    }

    #[test]
    fn only_changed_services_produce_a_row() {
        let urls = DevUrls::defaults(true, true, true);
        let before = starting_snapshot(&urls);
        let after = BannerSnapshot {
            api: Some(ServiceState::Ready),
            ..before.clone()
        };
        let changes = transitions(&urls, &before, &after);
        assert_eq!(
            changes,
            vec![Transition {
                name: "api".into(),
                state: ServiceState::Ready,
            }]
        );
        let text = render_transitions(ui::Face::PLAIN, &changes, &[]);
        assert_eq!(text, "  ok    api      ready\n");
    }

    #[test]
    fn an_unchanged_snapshot_prints_nothing() {
        let urls = DevUrls::defaults(true, true, true);
        let snap = starting_snapshot(&urls);
        assert!(transitions(&urls, &snap, &snap).is_empty());
        assert_eq!(render_transitions(ui::Face::PLAIN, &[], &[]), "");
    }

    #[test]
    fn a_regression_to_starting_is_reported_without_an_ok_marker() {
        let urls = DevUrls::defaults(true, false, false);
        let ready = BannerSnapshot {
            api: Some(ServiceState::Ready),
            app: None,
            www: None,
            prometheus: Some(ServiceState::Ready),
            tempo: Some(ServiceState::Ready),
            loki: Some(ServiceState::Ready),
            monitoring: Some(ServiceState::Ready),
            console: None,
            admin: None,
            extra: vec![],
        };
        let restarting = BannerSnapshot {
            api: Some(ServiceState::Starting),
            ..ready.clone()
        };
        let text = render_transitions(
            ui::Face::PLAIN,
            &transitions(&urls, &ready, &restarting),
            &[],
        );
        assert_eq!(text, "        api      starting\n");
        assert!(urls.api.is_some());
    }

    #[test]
    fn transition_rows_share_one_state_column() {
        let urls = DevUrls::defaults(true, true, true);
        let before = starting_snapshot(&urls);
        let after = snapshot(ServiceState::Ready, &urls);
        let text = render_transitions(ui::Face::PLAIN, &transitions(&urls, &before, &after), &[]);
        let offsets: Vec<usize> = text.lines().filter_map(|l| l.find("ready")).collect();
        assert_eq!(offsets.len(), 9);
        assert!(
            offsets.windows(2).all(|w| w[0] == w[1]),
            "state column ragged: {offsets:?}\n{text}"
        );
        // `console` is the widest name, so every row is padded to it.
        assert!(text.contains("console  ready"));
    }

    #[test]
    fn transitions_have_no_escapes_when_colour_is_off() {
        let urls = DevUrls::defaults(true, true, true);
        let before = starting_snapshot(&urls);
        let after = BannerSnapshot {
            api: Some(ServiceState::Migrating),
            app: Some(ServiceState::Ready),
            ..before.clone()
        };
        let text = render_transitions(ui::Face::PLAIN, &transitions(&urls, &before, &after), &[]);
        assert!(!text.contains('\u{1b}'));
        let coloured = ui::Face {
            color: true,
            emoji: false,
        };
        assert_eq!(
            ui::strip_ansi(&render_transitions(
                coloured,
                &transitions(&urls, &before, &after),
                &[]
            )),
            text,
        );
    }

    #[test]
    fn banner_omits_prometheus_when_disabled() {
        let mut urls = DevUrls::defaults(true, false, false);
        urls.prometheus = None;
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        assert!(text.contains("api"));
        assert!(!text.contains("prom"));
        assert!(!text.contains("9090"));
    }

    #[test]
    fn banner_omits_tempo_when_disabled() {
        let mut urls = DevUrls::defaults(true, false, false);
        urls.tempo = None;
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        assert!(text.contains("api"));
        assert!(!text.contains("tempo"));
        assert!(!text.contains("3200"));
    }

    #[test]
    fn banner_omits_loki_when_disabled() {
        let mut urls = DevUrls::defaults(true, false, false);
        urls.loki = None;
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        assert!(text.contains("api"));
        assert!(!text.contains("loki"));
        assert!(!text.contains("3100"));
    }

    #[test]
    fn extra_service_row_follows_admin() {
        let mut urls = DevUrls::defaults(true, false, false);
        urls.extra.push((
            "vision".into(),
            "http://localhost:8765/tools/solve_studio/".into(),
        ));
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        let admin = text.find("admin").expect("admin row");
        let vision = text.find("vision").expect("vision row");
        assert!(vision > admin, "{text}");
        assert!(text.contains("http://localhost:8765/tools/solve_studio/"));
    }

    #[test]
    fn extra_service_row_is_omitted_when_empty() {
        let urls = DevUrls::defaults(true, false, false);
        let text = render_banner_when(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        assert!(!text.contains("vision"));
        assert!(!text.contains("8765"));
    }

    #[test]
    fn extra_row_does_not_change_pinned_height() {
        let mut urls = DevUrls::defaults(true, false, false);
        urls.extra
            .push(("vision".into(), "http://localhost:8765/".into()));
        let starting = region_lines(ui::Face::PLAIN, &urls, &starting_snapshot(&urls));
        let mut ready = starting_snapshot(&urls);
        ready.extra = vec![ServiceState::Ready];
        let ready_lines = region_lines(ui::Face::PLAIN, &urls, &ready);
        assert_eq!(starting.len(), ready_lines.len());
    }

    #[test]
    fn extra_transition_uses_the_package_name() {
        let mut urls = DevUrls::defaults(true, false, false);
        urls.extra
            .push(("vision".into(), "http://localhost:8765/".into()));
        let before = starting_snapshot(&urls);
        let mut after = before.clone();
        after.extra = vec![ServiceState::Ready];
        let changes = transitions(&urls, &before, &after);
        assert_eq!(
            changes,
            vec![Transition {
                name: "vision".into(),
                state: ServiceState::Ready,
            }]
        );
        let names = ["vision"];
        let text = render_transitions(ui::Face::PLAIN, &changes, &names);
        assert!(text.contains("vision"));
        assert!(text.contains("ready"));
    }
}
