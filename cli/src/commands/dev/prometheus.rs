use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tokio::process::Command;

use super::log::LogSink;
use super::process::spawn_labeled;

const PROMETHEUS_YML: &str = include_str!("../../../templates/prometheus/prometheus.yml");

pub const LISTEN_ADDR: &str = "127.0.0.1:9090";
pub const LISTEN_URL: &str = "http://localhost:9090";

pub fn binary_on_path() -> bool {
    StdCommand::new("prometheus")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Which process a scrape target belongs to, and how to reach it.
pub struct ScrapeTarget<'a> {
    /// Job name in the generated config.
    pub job: &'a str,
    /// Port it listens on.
    pub port: u16,
    /// Bearer token, when `/metrics` is gated.
    pub bearer_token: Option<&'a str>,
}

/// Default port the monitoring collector listens on in development.
pub const MONITORING_PORT: u16 = 3001;

/// Write a Prometheus config for a development run.
///
/// Scrapes the application, and the monitoring collector too when this project
/// has one — an operator needs to know when the thing doing the watching is
/// itself struggling. A project without a `monitoring/` directory does not get
/// a target pointed at a port nothing is listening on: a permanently red scrape
/// target teaches people to ignore the scrape health page.
pub fn prepare_dir(
    root: &Path,
    api_port: u16,
    bearer_token: Option<&str>,
) -> std::io::Result<PathBuf> {
    let mut targets = vec![ScrapeTarget {
        job: "erno-api",
        port: api_port,
        bearer_token,
    }];

    if root.join("monitoring").join("Cargo.toml").exists() {
        targets.push(ScrapeTarget {
            job: "erno-monitoring",
            port: MONITORING_PORT,
            // The collector's own `/metrics` is ungated in development.
            bearer_token: None,
        });
    }

    write_config(root, &targets)
}

/// Write a Prometheus config that scrapes exactly the given targets.
pub fn write_config(root: &Path, targets: &[ScrapeTarget<'_>]) -> std::io::Result<PathBuf> {
    let dir = root.join(".erno").join("prometheus");
    std::fs::create_dir_all(dir.join("data"))?;
    std::fs::write(dir.join("prometheus.yml"), render_config(targets))?;
    Ok(dir)
}

/// Build the config text. Split out so it can be tested without a filesystem.
pub fn render_config(targets: &[ScrapeTarget<'_>]) -> String {
    let header = PROMETHEUS_YML
        .split("scrape_configs:")
        .next()
        .unwrap_or("")
        .to_string();

    let mut yml = header;
    yml.push_str("scrape_configs:\n");

    for target in targets {
        yml.push_str(&format!("  - job_name: {}\n", target.job));
        yml.push_str("    metrics_path: /metrics\n");
        if let Some(token) = target.bearer_token.filter(|t| !t.is_empty()) {
            yml.push_str(&format!("    bearer_token: {token}\n"));
        }
        yml.push_str("    static_configs:\n");
        yml.push_str(&format!(
            "      - targets: [\"127.0.0.1:{}\"]\n",
            target.port
        ));
    }

    yml
}

pub fn spawn(dir: &Path, sink: std::sync::Arc<LogSink>) -> tokio::process::Child {
    let mut cmd = Command::new("prometheus");
    cmd.arg(format!(
        "--config.file={}",
        dir.join("prometheus.yml").display()
    ));
    cmd.arg(format!(
        "--storage.tsdb.path={}",
        dir.join("data").display()
    ));
    cmd.arg(format!("--web.listen-address={LISTEN_ADDR}"));
    cmd.arg("--storage.tsdb.retention.time=15d");
    spawn_labeled(cmd, dir, "prom", sink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_target_is_rendered() {
        let config = render_config(&[ScrapeTarget {
            job: "erno-api",
            port: 3000,
            bearer_token: None,
        }]);

        assert!(config.contains("job_name: erno-api"));
        assert!(config.contains("127.0.0.1:3000"));
        assert!(!config.contains("bearer_token"));
        assert!(
            config.contains("scrape_interval"),
            "the global header survives"
        );
    }

    #[test]
    fn both_the_application_and_the_collector_are_scraped() {
        let config = render_config(&[
            ScrapeTarget {
                job: "erno-api",
                port: 3000,
                bearer_token: None,
            },
            ScrapeTarget {
                job: "erno-monitoring",
                port: 3001,
                bearer_token: None,
            },
        ]);

        assert!(config.contains("job_name: erno-api"));
        assert!(config.contains("job_name: erno-monitoring"));
        assert!(config.contains("127.0.0.1:3001"));
    }

    #[test]
    fn a_gated_metrics_endpoint_gets_its_token() {
        let config = render_config(&[ScrapeTarget {
            job: "erno-api",
            port: 3000,
            bearer_token: Some("secret-token"),
        }]);
        assert!(config.contains("bearer_token: secret-token"));
    }

    #[test]
    fn an_empty_token_is_treated_as_absent() {
        // An empty string in config means "not set", not "authenticate with
        // nothing" — which Prometheus would send as a literal empty header.
        let config = render_config(&[ScrapeTarget {
            job: "erno-api",
            port: 3000,
            bearer_token: Some(""),
        }]);
        assert!(!config.contains("bearer_token"));
    }

    #[test]
    fn no_targets_still_produces_a_valid_document() {
        let config = render_config(&[]);
        assert!(config.contains("scrape_configs:"));
        assert!(config.contains("global:"));
    }
}
