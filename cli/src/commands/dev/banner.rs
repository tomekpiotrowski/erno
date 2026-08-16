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
    fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Migrating => "migrating",
            Self::Ready => "ready",
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
}

impl DevUrls {
    #[cfg(test)]
    pub fn defaults(start_api: bool, start_app: bool, start_www: bool) -> Self {
        Self {
            api: start_api.then(|| "http://localhost:3000".to_string()),
            app: start_app.then(|| "http://localhost:4200".to_string()),
            www: start_www.then(|| "http://localhost:4321".to_string()),
            prometheus: start_api.then(|| super::prometheus::LISTEN_URL.to_string()),
        }
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

    pub fn prometheus_ready(&self) -> Option<String> {
        self.prometheus
            .as_deref()
            .map(|u| format!("{}/-/ready", u.trim_end_matches('/')))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BannerSnapshot {
    pub api: Option<ServiceState>,
    pub app: Option<ServiceState>,
    pub www: Option<ServiceState>,
    pub prometheus: Option<ServiceState>,
}

/// One row of the banner: a service (with a readiness state) or a hidden
/// surface (with a static note).
struct BannerRow {
    name: &'static str,
    url: String,
    state: Option<ServiceState>,
    note: Option<&'static str>,
}

fn banner_rows(urls: &DevUrls, snap: &BannerSnapshot) -> (Vec<BannerRow>, Vec<BannerRow>) {
    let mut services = Vec::new();
    let mut surfaces = Vec::new();

    let mut push = |name, url: &str, state| {
        services.push(BannerRow {
            name,
            url: url.to_string(),
            state: Some(state),
            note: None,
        })
    };
    if let (Some(url), Some(state)) = (urls.www.as_deref(), snap.www) {
        push("www", url, state);
    }
    if let (Some(url), Some(state)) = (urls.app.as_deref(), snap.app) {
        push("app", url, state);
    }
    if let (Some(url), Some(state)) = (urls.api.as_deref(), snap.api) {
        push("api", url, state);
    }
    if let (Some(url), Some(state)) = (urls.prometheus.as_deref(), snap.prometheus) {
        push("prom", url, state);
    }

    // The surfaces the API exposes but does not announce. Grouped after the
    // services rather than wedged between them.
    if let Some(api) = urls.api.as_deref() {
        let base = api.trim_end_matches('/');
        for (name, url, note) in [
            (
                "admin",
                "http://localhost:4300".to_string(),
                Some("password: admin"),
            ),
            ("mail", format!("{base}/dev/emails"), None),
            ("jobs", format!("{base}/dev/jobs"), None),
        ] {
            surfaces.push(BannerRow {
                name,
                url,
                state: None,
                note,
            });
        }
    }

    (services, surfaces)
}

pub fn render_banner(urls: &DevUrls, snap: &BannerSnapshot) -> String {
    render_banner_when(ui::color(), urls, snap)
}

pub fn render_banner_when(on: bool, urls: &DevUrls, snap: &BannerSnapshot) -> String {
    let (services, surfaces) = banner_rows(urls, snap);

    // Both columns are sized from the content, across every row, so the state
    // and note columns line up no matter how long the URLs are.
    let all = services.iter().chain(surfaces.iter());
    let name_w = ui::column_width(all.clone().map(|r| r.name));
    let url_w = ui::column_width(all.map(|r| r.url.as_str()));

    let row = |r: &BannerRow| {
        let tail = match (r.state, r.note) {
            (Some(state), _) => ui::paint_when(on, state.style(), state.label()),
            (None, Some(note)) => ui::paint_when(on, ui::DIM, note),
            (None, None) => String::new(),
        };
        let name = ui::paint_when(on, ui::label_style(r.name), &format!("{:<name_w$}", r.name));
        format!("  {name}  {:<url_w$}  {tail}\n", r.url)
            .trim_end()
            .to_string()
            + "\n"
    };

    let mut out = String::from("\n");
    out.push_str(&format!(
        "  {}\n",
        ui::paint_when(on, ui::DIM, "erno — Ctrl+C to stop")
    ));
    for r in &services {
        out.push_str(&row(r));
    }
    if !surfaces.is_empty() {
        out.push('\n');
        for r in &surfaces {
            out.push_str(&row(r));
        }
    }
    out.push('\n');
    out
}

pub fn print_banner(urls: &DevUrls, snap: &BannerSnapshot) {
    // Narration, like everything else the CLI says about itself.
    for line in render_banner(urls, snap).lines() {
        ui::emit(ui::Stream::Err, line);
    }
}

pub fn starting_snapshot(urls: &DevUrls) -> BannerSnapshot {
    BannerSnapshot {
        api: urls.api.as_ref().map(|_| ServiceState::Starting),
        app: urls.app.as_ref().map(|_| ServiceState::Starting),
        www: urls.www.as_ref().map(|_| ServiceState::Starting),
        prometheus: urls.prometheus.as_ref().map(|_| ServiceState::Starting),
    }
}

/// Probe HTTP endpoints and reprint the banner whenever a service changes state.
pub fn spawn_readiness_watcher(urls: DevUrls) {
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
                print_banner(&urls, &next);
                last = next;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });
}

async fn probe_all(client: &Client, urls: &DevUrls) -> BannerSnapshot {
    let api = match urls.api.as_deref() {
        Some(_) => Some(probe_api(client, urls).await),
        None => None,
    };
    let app = match urls.app.as_deref() {
        Some(url) => Some(probe_http(client, url).await),
        None => None,
    };
    let www = match urls.www.as_deref() {
        Some(url) => Some(probe_http(client, url).await),
        None => None,
    };
    let prometheus = match urls.prometheus_ready() {
        Some(url) => Some(probe_http(client, &url).await),
        None => None,
    };
    BannerSnapshot {
        api,
        app,
        www,
        prometheus,
    }
}

async fn probe_api(client: &Client, urls: &DevUrls) -> ServiceState {
    let Some(readiness) = urls.api_readiness() else {
        return ServiceState::Starting;
    };
    if is_up(client, &readiness).await {
        ServiceState::Ready
    } else if let Some(liveness) = urls.api_liveness() {
        if is_up(client, &liveness).await {
            ServiceState::Migrating
        } else {
            ServiceState::Starting
        }
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

    #[test]
    fn banner_includes_urls_and_states() {
        let urls = DevUrls::defaults(true, true, true);
        let snap = BannerSnapshot {
            api: Some(ServiceState::Ready),
            app: Some(ServiceState::Starting),
            www: Some(ServiceState::Ready),
            prometheus: Some(ServiceState::Ready),
        };
        let text = render_banner(&urls, &snap);
        assert!(text.contains("http://localhost:3000"));
        assert!(text.contains("http://localhost:4200"));
        assert!(text.contains("http://localhost:4321"));
        assert!(text.contains("http://localhost:9090"));
        assert!(text.contains("ready"));
        assert!(text.contains("starting"));
        assert!(text.contains("http://localhost:4300"));
        assert!(text.contains("/dev/emails"));
        assert!(text.contains("/dev/jobs"));
    }

    #[test]
    fn banner_shows_migrating() {
        let urls = DevUrls::defaults(true, true, false);
        let snap = BannerSnapshot {
            api: Some(ServiceState::Migrating),
            app: Some(ServiceState::Starting),
            www: None,
            prometheus: Some(ServiceState::Starting),
        };
        let text = render_banner(&urls, &snap);
        assert!(text.contains("migrating"));
        assert!(!text.contains("www"));
    }

    #[test]
    fn banner_omits_www_when_absent() {
        let urls = DevUrls::defaults(true, true, false);
        let snap = starting_snapshot(&urls);
        let text = render_banner(&urls, &snap);
        assert!(!text.contains("www"));
        assert!(text.contains("api"));
        assert!(urls.www.is_none());
    }

    #[test]
    fn hidden_surfaces_use_api_origin() {
        let mut urls = DevUrls::defaults(true, false, false);
        urls.api = Some("http://localhost:3010/".to_string());
        let text = render_banner_when(false, &urls, &starting_snapshot(&urls));
        assert!(text.contains("http://localhost:3010/dev/emails"));
        assert!(text.contains("http://localhost:3010/dev/jobs"));
        assert!(text.contains("password: admin"));
    }

    #[test]
    fn banner_has_no_escapes_when_colour_is_off() {
        let urls = DevUrls::defaults(true, true, true);
        let text = render_banner_when(false, &urls, &starting_snapshot(&urls));
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
        };
        let text = render_banner_when(false, &urls, &snap);

        let offsets: Vec<usize> = text
            .lines()
            .filter_map(|l| l.find("ready").or_else(|| l.find("starting")))
            .collect();
        assert!(
            offsets.len() >= 4,
            "expected a state word on every service row"
        );
        assert!(
            offsets.windows(2).all(|w| w[0] == w[1]),
            "state column ragged: {offsets:?}\n{text}"
        );
    }

    #[test]
    fn banner_omits_unselected_services() {
        let urls = DevUrls::defaults(true, false, false);
        let text = render_banner(&urls, &starting_snapshot(&urls));
        assert!(text.contains("api"));
        assert!(text.contains("prom"));
        assert!(text.contains("http://localhost:9090"));
        assert!(!text.contains("app"));
        assert!(!text.contains("www"));
    }

    #[test]
    fn banner_omits_prometheus_when_disabled() {
        let mut urls = DevUrls::defaults(true, false, false);
        urls.prometheus = None;
        let text = render_banner(&urls, &starting_snapshot(&urls));
        assert!(text.contains("api"));
        assert!(!text.contains("prom"));
        assert!(!text.contains("9090"));
    }
}
