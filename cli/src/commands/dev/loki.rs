use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tokio::process::Command;

use super::log::LogSink;
use super::process::spawn_labeled;

const LOKI_YML: &str = include_str!("../../../templates/loki/loki.yaml");

pub const LISTEN_URL: &str = "http://localhost:3100";
pub const GRPC_PORT: u16 = 9096;

/// What `loki` on PATH actually is.
///
/// Debian/Ubuntu ship an MCMC linkage-analysis binary also named `loki`. Its
/// `-version` exits 0, so a mere success check treats it as Grafana Loki and
/// `erno dev` restart-loops it with `-config.file`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Binary {
    Grafana { version: String },
    Missing,
    Other { summary: String },
}

pub(crate) fn probe() -> Binary {
    classify(version_output().as_deref())
}

fn version_output() -> Option<String> {
    for arg in ["-version", "--version"] {
        match StdCommand::new("loki").arg(arg).output() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => continue,
            Ok(out) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn classify(output: Option<&str>) -> Binary {
    match output {
        None => Binary::Missing,
        Some(text) if is_grafana_loki(text) => Binary::Grafana {
            version: summarize(text),
        },
        Some(text) => Binary::Other {
            summary: summarize(text),
        },
    }
}

fn is_grafana_loki(text: &str) -> bool {
    // Grafana dskit banner: "loki, version 3.4.2 (branch: HEAD, …)"
    text.to_ascii_lowercase().contains("loki, version")
}

fn summarize(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.contains("invalid option"))
        .unwrap_or("unknown")
        .to_string()
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
        assert!(config.contains("grpc_listen_port: 9096"));
        assert!(config.contains("/tmp/erno-loki/chunks"));
        assert!(!config.contains("__DATA__"));
    }

    #[test]
    fn grafana_loki_banner_is_accepted() {
        assert_eq!(
            classify(Some(
                "loki, version 3.4.2 (branch: HEAD, revision: abc)\n  go version: go1.23.3\n"
            )),
            Binary::Grafana {
                version: "loki, version 3.4.2 (branch: HEAD, revision: abc)".into(),
            }
        );
    }

    #[test]
    fn debian_mcmc_loki_is_rejected() {
        assert_eq!(
            classify(Some("loki 2.4.7_4\n")),
            Binary::Other {
                summary: "loki 2.4.7_4".into(),
            }
        );
        assert_eq!(
            classify(Some(
                "loki: invalid option -- 'c'\n[loki.c:254] No parameter file specified\n"
            )),
            Binary::Other {
                summary: "[loki.c:254] No parameter file specified".into(),
            }
        );
    }

    #[test]
    fn missing_loki_is_missing() {
        assert_eq!(classify(None), Binary::Missing);
    }
}
