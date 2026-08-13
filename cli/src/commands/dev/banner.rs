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
    pub api: String,
    pub app: String,
    pub www: Option<String>,
}

impl DevUrls {
    pub fn defaults(has_www: bool) -> Self {
        Self {
            api: "http://localhost:3000".to_string(),
            app: "http://localhost:4200".to_string(),
            www: has_www.then(|| "http://localhost:4321".to_string()),
        }
    }

    pub fn api_readiness(&self) -> String {
        format!("{}/readiness", self.api.trim_end_matches('/'))
    }

    pub fn api_liveness(&self) -> String {
        format!("{}/liveness", self.api.trim_end_matches('/'))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BannerSnapshot {
    pub api: Option<ServiceState>,
    pub app: Option<ServiceState>,
    pub www: Option<ServiceState>,
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
    if let Some(state) = snap.app {
        out.push_str(&format_row(GREEN, "app", &urls.app, state));
    }
    if let Some(state) = snap.api {
        out.push_str(&format_row(CYAN, "api", &urls.api, state));
    }
    out.push('\n');
    out
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
        api: Some(ServiceState::Starting),
        app: Some(ServiceState::Starting),
        www: urls.www.as_ref().map(|_| ServiceState::Starting),
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
    let api = Some(probe_api(client, urls).await);
    let app = Some(probe_http(client, &urls.app).await);
    let www = match &urls.www {
        Some(url) => Some(probe_http(client, url).await),
        None => None,
    };
    BannerSnapshot { api, app, www }
}

async fn probe_api(client: &Client, urls: &DevUrls) -> ServiceState {
    if is_up(client, &urls.api_readiness()).await {
        ServiceState::Ready
    } else if is_up(client, &urls.api_liveness()).await {
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

    #[test]
    fn banner_includes_urls_and_states() {
        let urls = DevUrls::defaults(true);
        let snap = BannerSnapshot {
            api: Some(ServiceState::Ready),
            app: Some(ServiceState::Starting),
            www: Some(ServiceState::Ready),
        };
        let text = render_banner(&urls, &snap);
        assert!(text.contains("http://localhost:3000"));
        assert!(text.contains("http://localhost:4200"));
        assert!(text.contains("http://localhost:4321"));
        assert!(text.contains("ready"));
        assert!(text.contains("starting"));
    }

    #[test]
    fn banner_shows_migrating() {
        let urls = DevUrls::defaults(false);
        let snap = BannerSnapshot {
            api: Some(ServiceState::Migrating),
            app: Some(ServiceState::Starting),
            www: None,
        };
        let text = render_banner(&urls, &snap);
        assert!(text.contains("migrating"));
        assert!(!text.contains("www"));
    }

    #[test]
    fn banner_omits_www_when_absent() {
        let urls = DevUrls::defaults(false);
        let snap = starting_snapshot(&urls);
        let text = render_banner(&urls, &snap);
        assert!(!text.contains("www"));
        assert!(text.contains("api"));
        assert!(urls.www.is_none());
    }
}
