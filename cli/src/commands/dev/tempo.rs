use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tokio::process::Command;

use super::log::LogSink;
use super::process::spawn_labeled;

const TEMPO_YML: &str = include_str!("../../../templates/tempo/tempo.yaml");

pub const LISTEN_URL: &str = "http://localhost:3200";
pub const OTLP_PORT: u16 = 4318;
pub const GRPC_PORT: u16 = 9095;

pub fn binary_on_path() -> bool {
    for arg in ["-version", "--version"] {
        if StdCommand::new("tempo")
            .arg(arg)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

/// Render the config with `data` substituted for `__DATA__`.
pub fn render_config(data: &Path) -> String {
    TEMPO_YML.replace("__DATA__", &data.display().to_string())
}

pub fn prepare_dir(root: &Path) -> std::io::Result<PathBuf> {
    let dir = root.join(".erno").join("tempo");
    std::fs::create_dir_all(dir.join("wal"))?;
    std::fs::create_dir_all(dir.join("blocks"))?;
    std::fs::create_dir_all(dir.join("live-store").join("traces"))?;
    std::fs::create_dir_all(dir.join("live-store").join("shutdown-marker"))?;
    std::fs::create_dir_all(dir.join("work"))?;
    std::fs::write(dir.join("tempo.yaml"), render_config(&dir))?;
    Ok(dir)
}

pub fn spawn(dir: &Path, sink: std::sync::Arc<LogSink>) -> tokio::process::Child {
    let mut cmd = Command::new("tempo");
    cmd.arg(format!("-config.file={}", dir.join("tempo.yaml").display()));
    spawn_labeled(cmd, dir, "tempo", sink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_config_is_a_local_otlp_store() {
        let config = render_config(Path::new("/tmp/erno-tempo"));
        assert!(config.contains("backend: local"));
        assert!(config.contains("path: /tmp/erno-tempo/wal"));
        assert!(config.contains("path: /tmp/erno-tempo/blocks"));
        assert!(config.contains("path: /tmp/erno-tempo/live-store/traces"));
        assert!(config.contains("local_work_path: /tmp/erno-tempo/work"));
        assert!(config.contains("http_listen_port: 3200"));
        assert!(config.contains("grpc_listen_port: 9095"));
        assert!(config.contains("endpoint: 127.0.0.1:4318"));
        assert!(config.contains("live_store:"));
        assert!(!config.contains("\ningester:"));
        assert!(!config.contains("\ncompactor:"));
        assert!(!config.contains("__DATA__"));
    }
}
