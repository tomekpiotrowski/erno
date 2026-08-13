use std::time::Duration;

use reqwest::Client;

use super::{CYAN, DIM, GREEN, MAGENTA, RESET, YELLOW};

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

    fn color(self) -> &'static str {
        match self {
            Self::Starting => YELLOW,
            Self::Migrating => YELLOW,
            Self::Ready => GREEN,
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

pub fn render_banner(urls: &DevUrls, snap: &BannerSnapshot) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!(
        "  {DIM}Erno{RESET}                                   Ctrl+C to stop\n"
    ));
    if let (Some(url), Some(state)) = (urls.www.as_deref(), snap.www) {
        out.push_str(&format_row(MAGENTA, "www", url, state));
    }
    if let (Some(url), Some(state)) = (urls.app.as_deref(), snap.app) {
        out.push_str(&format_row(GREEN, "app", url, state));
    }
    if let (Some(url), Some(state)) = (urls.api.as_deref(), snap.api) {
        out.push_str(&format_row(CYAN, "api", url, state));
        out.push_str(&hidden_surfaces(url));
    }
    if let (Some(url), Some(state)) = (urls.prometheus.as_deref(), snap.prometheus) {
        out.push_str(&format_row(YELLOW, "prom", url, state));
    }
    out.push('\n');
    out
}

pub fn hidden_surfaces(api_url: &str) -> String {
    let base = api_url.trim_end_matches('/');
    format!(
        "  {DIM}admin{RESET} http://localhost:4300             password: admin\n  {DIM}mail {RESET} {base}/dev/emails\n  {DIM}jobs {RESET} {base}/dev/jobs\n"
    )
}

fn format_row(color: &str, name: &str, url: &str, state: ServiceState) -> String {
    format!(
        "  {color}{name:<5}{RESET} {url:<36} {sc}{label}{RESET}\n",
        sc = state.color(),
        label = state.label(),
    )
}

pub fn print_banner(urls: &DevUrls, snap: &BannerSnapshot) {
    print!("{}", render_banner(urls, snap));
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
        let text = hidden_surfaces("http://localhost:3010/");
        assert!(text.contains("http://localhost:3010/dev/emails"));
        assert!(text.contains("password: admin"));
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
