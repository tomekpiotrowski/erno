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

pub fn prepare_dir(
    root: &Path,
    api_port: u16,
    bearer_token: Option<&str>,
) -> std::io::Result<PathBuf> {
    let dir = root.join(".erno").join("prometheus");
    std::fs::create_dir_all(dir.join("data"))?;
    let mut yml = PROMETHEUS_YML.replace("127.0.0.1:3000", &format!("127.0.0.1:{api_port}"));
    if let Some(token) = bearer_token.filter(|t| !t.is_empty()) {
        yml = yml.replacen(
            "    static_configs:",
            &format!("    bearer_token: {token}\n    static_configs:"),
            1,
        );
    }
    std::fs::write(dir.join("prometheus.yml"), yml)?;
    Ok(dir)
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
