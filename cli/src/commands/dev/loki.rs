use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tokio::process::Command;

use super::log::LogSink;
use super::process::spawn_labeled;

const LOKI_YML: &str = include_str!("../../../templates/loki/loki.yaml");

pub const LISTEN_URL: &str = "http://localhost:3100";

pub fn binary_on_path() -> bool {
    for arg in ["-version", "--version"] {
        if StdCommand::new("loki")
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
    LOKI_YML.replace("__DATA__", &data.display().to_string())
}

pub fn prepare_dir(root: &Path) -> std::io::Result<PathBuf> {
    let dir = root.join(".erno").join("loki");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("loki.yaml"), render_config(&dir))?;
    Ok(dir)
}

pub fn spawn(dir: &Path, sink: std::sync::Arc<LogSink>) -> tokio::process::Child {
    let mut cmd = Command::new("loki");
    cmd.arg(format!("-config.file={}", dir.join("loki.yaml").display()));
    spawn_labeled(cmd, dir, "loki", sink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_config_is_single_tenant_filesystem() {
        let config = render_config(Path::new("/tmp/erno-loki"));
        assert!(config.contains("auth_enabled: false"));
        assert!(config.contains("allow_structured_metadata: true"));
        assert!(config.contains("http_listen_port: 3100"));
        assert!(config.contains("/tmp/erno-loki/chunks"));
        assert!(!config.contains("__DATA__"));
    }
}
