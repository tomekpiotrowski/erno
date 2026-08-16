use std::process::Command;
use std::time::Duration;

use reqwest::Client;

use crate::ui;

pub fn url_to_open(www: Option<&str>, app: Option<&str>, api: Option<&str>) -> Option<String> {
    www.or(app).or(api).map(|s| s.to_string())
}

pub fn spawn_opener(url: String) {
    tokio::spawn(async move {
        if wait_for_http(&url).await {
            match open_browser(&url) {
                Ok(()) => ui::info(format!("Opened {url}")),
                Err(e) => ui::warn(format!("could not open {url}: {e}")),
            }
        } else {
            ui::warn(format!(
                "timed out waiting for {url} — not opening a browser"
            ));
        }
    });
}

async fn wait_for_http(url: &str) -> bool {
    let client = match Client::builder()
        .timeout(Duration::from_millis(500))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    for _ in 0..600 {
        if let Ok(res) = client.get(url).send().await {
            if res.status().as_u16() < 500 {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

pub fn open_browser(url: &str) -> Result<(), String> {
    let mut cmd = open_command(url);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn open_command(url: &str) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(url);
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", url]);
        cmd
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(url);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_www_then_app_then_api() {
        assert_eq!(
            url_to_open(Some("http://w"), Some("http://a"), Some("http://api")).as_deref(),
            Some("http://w")
        );
        assert_eq!(
            url_to_open(None, Some("http://a"), Some("http://api")).as_deref(),
            Some("http://a")
        );
        assert_eq!(
            url_to_open(None, None, Some("http://api")).as_deref(),
            Some("http://api")
        );
        assert_eq!(url_to_open(None, None, None), None);
    }
}
