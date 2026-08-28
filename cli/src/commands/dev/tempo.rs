use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::Arc;

use tokio::process::{Child, Command};

use super::log::LogSink;
use super::process::spawn_labeled;
use super::project::absolute_dir;

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
    let dir = dir.canonicalize()?;
    std::fs::write(dir.join("tempo.yaml"), render_config(&dir))?;
    Ok(dir)
}

pub(crate) fn spawn_args(dir: &Path) -> Vec<String> {
    let dir = absolute_dir(dir);
    vec![format!("-config.file={}", dir.join("tempo.yaml").display())]
}

pub fn spawn(dir: &Path, sink: Arc<LogSink>) -> Child {
    let dir = absolute_dir(dir);
    let mut cmd = Command::new("tempo");
    cmd.args(spawn_args(&dir));
    spawn_labeled(cmd, &dir, "tempo", sink)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::absolute;

    #[test]
    fn rendered_config_is_a_local_otlp_store() {
        let config = render_config(Path::new("/tmp/erno-tempo"));
        assert!(config.contains("backend: local"));
        assert!(config.contains("path: /tmp/erno-tempo/wal"));
        assert!(config.contains("path: /tmp/erno-tempo/blocks"));
        assert!(config.contains("path: /tmp/erno-tempo/live-store/traces"));
        assert!(config.contains("local_work_path: /tmp/erno-tempo/work"));
        assert!(config.contains("http_listen_port: 3200"));
        assert!(config.contains(&format!("grpc_listen_port: {GRPC_PORT}")));
        assert!(config.contains("endpoint: 127.0.0.1:4318"));
        assert!(config.contains("live_store:"));
        assert!(!config.contains("\ningester:"));
        assert!(!config.contains("\ncompactor:"));
        assert!(!config.contains("__DATA__"));
    }

    #[test]
    fn spawn_args_are_absolute_when_the_data_dir_is_a_relative_project_path() {
        let dir = PathBuf::from("teryon").join(".erno").join("tempo");
        let args = spawn_args(&dir);
        assert_eq!(
            flag_path(&args, "-config.file="),
            absolute(dir.join("tempo.yaml")).unwrap()
        );
    }

    #[test]
    fn a_relative_data_dir_is_absolute_in_the_rendered_config() {
        let dir = absolute_dir(Path::new("teryon/.erno/tempo"));
        let config = render_config(&dir);
        assert_eq!(dir, absolute(Path::new("teryon/.erno/tempo")).unwrap());
        assert!(config.contains(&format!("{}/wal", dir.display())));
        assert!(config.contains(&format!("{}/blocks", dir.display())));
        assert!(!config.contains("__DATA__"));
    }

    #[test]
    fn prepare_dir_writes_absolute_paths_into_the_config() {
        let tmp = std::env::temp_dir().join(format!(
            "erno-tempo-abs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let dir = prepare_dir(&tmp).unwrap();
        let yaml = std::fs::read_to_string(dir.join("tempo.yaml")).unwrap();
        assert!(dir.is_absolute(), "{}", dir.display());
        assert!(yaml.contains(&format!("{}/wal", dir.display())));
        assert!(!yaml.contains("__DATA__"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn flag_path(args: &[String], prefix: &str) -> PathBuf {
        let arg = args
            .iter()
            .find(|a| a.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing {prefix} in {args:?}"));
        PathBuf::from(arg.strip_prefix(prefix).unwrap())
    }
}
