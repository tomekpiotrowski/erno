//! `deploy/config.toml` and decrypted `deploy/secrets.<env>.yaml`.
//!
//! Hosts, replica counts, and image tags are not secrets. Image tags come from
//! the `erno deploy install <version>` argument; hosts live in config.toml.
//! A secrets file that still looks like a Helm values file is rejected so the
//! operator runs `erno deploy migrate` instead of shipping an empty image tag.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::Target;

pub const API_PORT: i32 = 3000;
pub const COLLECTOR_PORT: i32 = 3001;
pub const HTTP_PORT: i32 = 80;
pub const PROMETHEUS_PORT: i32 = 9090;
pub const TEMPO_PORT: i32 = 3200;
pub const TEMPO_OTLP_PORT: i32 = 4318;
pub const LOKI_PORT: i32 = 3100;
pub const DEFAULT_PROMETHEUS_IMAGE: &str = "prom/prometheus:v2.55.1";
pub const DEFAULT_TEMPO_IMAGE: &str = "grafana/tempo:3.0.3";
pub const DEFAULT_LOKI_IMAGE: &str = "grafana/loki:3.4.2";

/// Where the user-owned deploy files live for a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub target: Target,
    pub dir: &'static str,
}

impl Layout {
    pub fn for_target(target: Target) -> Self {
        Self {
            target,
            dir: match target {
                Target::App => "deploy",
                Target::Monitoring => "monitoring/deploy",
            },
        }
    }

    pub fn config_path(&self) -> PathBuf {
        Path::new(self.dir).join("config.toml")
    }

    pub fn secrets_path(&self, env: &str) -> PathBuf {
        Path::new(self.dir).join(format!("secrets.{env}.yaml"))
    }

    pub fn secrets_example(&self) -> PathBuf {
        Path::new(self.dir).join("secrets.example.yaml")
    }

    pub fn sops_path(&self) -> PathBuf {
        Path::new(self.dir).join(".sops.yaml")
    }

    pub fn extra_dir(&self) -> PathBuf {
        Path::new(self.dir).join("extra")
    }

    pub fn config_exists(&self) -> bool {
        self.config_path().exists()
    }

    /// Pre-migration Helm tree. Install refuses this without `deploy migrate`.
    pub fn legacy_chart_dir(&self) -> &'static str {
        match self.target {
            Target::App => "chart",
            Target::Monitoring => "monitoring/deploy/chart",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeployFile {
    pub github_repo: String,
    #[serde(flatten)]
    pub envs: BTreeMap<String, EnvConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvConfig {
    pub kubernetes_context: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default)]
    pub monitoring_url: String,
    #[serde(default)]
    pub hosts: Hosts,
    #[serde(default)]
    pub tls: Tls,
    #[serde(default)]
    pub workloads: Workloads,
    #[serde(default)]
    pub scrape: Scrape,
    #[serde(default)]
    pub prometheus: Prometheus,
    #[serde(default)]
    pub tempo: Tempo,
    #[serde(default)]
    pub loki: Loki,
    #[serde(default)]
    pub ingress: Ingress,
    /// How `erno deploy setup` installs ingress-nginx: `cloud` (LoadBalancer,
    /// the default), `kind`, or `baremetal`.
    #[serde(default)]
    pub ingress_provider: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Hosts {
    pub api: String,
    pub app: String,
    pub www: String,
    pub admin: String,
    pub monitoring: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tls {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_issuer")]
    pub issuer: String,
    #[serde(default)]
    pub email: String,
}

impl Default for Tls {
    fn default() -> Self {
        Self {
            enabled: true,
            issuer: default_issuer(),
            email: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workloads {
    #[serde(default = "default_true")]
    pub admin: bool,
    #[serde(default = "default_true")]
    pub www: bool,
    #[serde(default = "default_one")]
    pub api_replicas: i32,
    #[serde(default = "default_one")]
    pub app_replicas: i32,
    #[serde(default = "default_one")]
    pub www_replicas: i32,
    #[serde(default = "default_one")]
    pub admin_replicas: i32,
    #[serde(default = "default_one")]
    pub collector_replicas: i32,
    #[serde(default = "default_one")]
    pub console_replicas: i32,
}

impl Default for Workloads {
    fn default() -> Self {
        Self {
            admin: true,
            www: true,
            api_replicas: 1,
            app_replicas: 1,
            www_replicas: 1,
            admin_replicas: 1,
            collector_replicas: 1,
            console_replicas: 1,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Scrape {
    pub target: String,
    pub scheme: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prometheus {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_prom_image")]
    pub image: String,
    #[serde(default = "default_prom_retention")]
    pub retention: String,
    #[serde(default = "default_prom_storage")]
    pub storage: String,
}

impl Default for Prometheus {
    fn default() -> Self {
        Self {
            enabled: true,
            image: default_prom_image(),
            retention: default_prom_retention(),
            storage: default_prom_storage(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Ingress {
    /// 0 omits the nginx limit-rps annotation. Monitoring defaults to 20.
    pub rate_limit_rps: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub server: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSecrets {
    pub registry: Registry,
    pub api: AppApiSecrets,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppApiSecrets {
    pub database_url: String,
    pub jwt_secret: String,
    #[serde(default)]
    pub admin_password_hash: String,
    #[serde(default)]
    pub metrics_auth_token: String,
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default)]
    pub smtp_username: String,
    #[serde(default)]
    pub smtp_password: String,
    #[serde(default)]
    pub smtp_from: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default)]
    pub ingest_token: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitoringSecrets {
    pub registry: Registry,
    pub collector: CollectorSecrets,
    #[serde(default)]
    pub api: MonitoringApiSecrets,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectorSecrets {
    pub database_url: String,
    pub jwt_secret: String,
    #[serde(default = "default_admin_user")]
    pub admin_username: String,
    #[serde(default)]
    pub admin_password_hash: String,
    #[serde(default)]
    pub server_token: String,
    #[serde(default)]
    pub browser_token: String,
    #[serde(default)]
    pub metrics_auth_token: String,
    #[serde(default)]
    pub alerts_recipient: String,
    #[serde(default)]
    pub status_name: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default)]
    pub smtp_username: String,
    #[serde(default)]
    pub smtp_password: String,
    #[serde(default)]
    pub smtp_from: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct MonitoringApiSecrets {
    pub metrics_auth_token: String,
}

fn default_true() -> bool {
    true
}
fn default_one() -> i32 {
    1
}
fn default_namespace() -> String {
    "default".into()
}
fn default_issuer() -> String {
    "letsencrypt".into()
}
fn default_smtp_port() -> u16 {
    587
}
fn default_log_level() -> String {
    "info".into()
}
fn default_admin_user() -> String {
    "admin".into()
}
fn default_prom_image() -> String {
    DEFAULT_PROMETHEUS_IMAGE.into()
}
fn default_prom_retention() -> String {
    "90d".into()
}
fn default_prom_storage() -> String {
    "10Gi".into()
}

/// Trace store. Same knobs as Prometheus; default retention is shorter because
/// traces are fatter than samples.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tempo {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_tempo_image")]
    pub image: String,
    #[serde(default = "default_tempo_retention")]
    pub retention: String,
    #[serde(default = "default_tempo_storage")]
    pub storage: String,
}

impl Default for Tempo {
    fn default() -> Self {
        Self {
            enabled: true,
            image: default_tempo_image(),
            retention: default_tempo_retention(),
            storage: default_tempo_storage(),
        }
    }
}

/// Log store. Same knobs as Prometheus.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loki {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_loki_image")]
    pub image: String,
    #[serde(default = "default_loki_retention")]
    pub retention: String,
    #[serde(default = "default_loki_storage")]
    pub storage: String,
}

impl Default for Loki {
    fn default() -> Self {
        Self {
            enabled: true,
            image: default_loki_image(),
            retention: default_loki_retention(),
            storage: default_loki_storage(),
        }
    }
}

fn default_tempo_image() -> String {
    DEFAULT_TEMPO_IMAGE.into()
}
fn default_tempo_retention() -> String {
    "72h".into()
}
fn default_tempo_storage() -> String {
    "10Gi".into()
}
fn default_loki_image() -> String {
    DEFAULT_LOKI_IMAGE.into()
}
fn default_loki_retention() -> String {
    "7d".into()
}
fn default_loki_storage() -> String {
    "10Gi".into()
}

pub fn parse_deploy_file(toml: &str) -> Result<DeployFile, String> {
    toml::from_str(toml).map_err(|e| format!("invalid deploy/config.toml: {e}"))
}

pub fn env<'a>(file: &'a DeployFile, name: &str) -> Result<&'a EnvConfig, String> {
    file.envs.get(name).ok_or_else(|| {
        format!("no [{name}] section in deploy/config.toml — add the environment or pass --env")
    })
}

impl EnvConfig {
    pub fn validate(&self, target: Target) -> Result<(), String> {
        if self.kubernetes_context.trim().is_empty() {
            return Err("kubernetes_context is empty".into());
        }
        if self.namespace.trim().is_empty() {
            return Err("namespace is empty".into());
        }
        match target {
            Target::App => {
                require_host("hosts.api", &self.hosts.api)?;
                require_host("hosts.app", &self.hosts.app)?;
                if self.workloads.www {
                    require_host("hosts.www", &self.hosts.www)?;
                }
                if self.workloads.admin {
                    require_host("hosts.admin", &self.hosts.admin)?;
                }
            }
            Target::Monitoring => {
                require_host("hosts.monitoring", &self.hosts.monitoring)?;
                if self.prometheus.enabled && self.scrape.target.trim().is_empty() {
                    return Err(
                        "prometheus is enabled but scrape.target is empty (host:port to scrape)"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
}

fn require_host(key: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{key} is empty"))
    } else {
        Ok(())
    }
}

/// Image tag written onto every workload. `mon-v1.2.3` (the monitoring workflow
/// tag) stores the image as `v1.2.3`.
pub fn image_tag(version: &str, target: Target) -> String {
    let v = version.trim();
    match target {
        Target::Monitoring => v.strip_prefix("mon-").unwrap_or(v).to_string(),
        Target::App => v.to_string(),
    }
}

pub fn origin(tls: bool, host: &str) -> String {
    format!("{}://{host}", if tls { "https" } else { "http" })
}

pub fn looks_like_helm_values(yaml: &str) -> bool {
    yaml.lines().any(|line| {
        let t = line.trim();
        t.starts_with("imageTag:")
            || t.starts_with("ingress:")
            || t.starts_with("collector_url:")
            || t.starts_with("error_reporting:")
    })
}

pub fn parse_app_secrets(yaml: &str) -> Result<AppSecrets, String> {
    reject_helm_values(yaml)?;
    serde_yaml::from_str(yaml).map_err(|e| format!("invalid app secrets: {e}"))
}

pub fn parse_monitoring_secrets(yaml: &str) -> Result<MonitoringSecrets, String> {
    reject_helm_values(yaml)?;
    serde_yaml::from_str(yaml).map_err(|e| format!("invalid monitoring secrets: {e}"))
}

fn reject_helm_values(yaml: &str) -> Result<(), String> {
    if looks_like_helm_values(yaml) {
        Err("this secrets file is still in Helm chart form\n\
             Run `erno deploy migrate` to convert chart/ into deploy/."
            .into())
    } else {
        Ok(())
    }
}

/// Decrypt with `sops -d`. Never writes plaintext to disk.
pub fn decrypt_sops(path: &Path) -> Result<String, String> {
    let output = std::process::Command::new("sops")
        .args(["-d", &path.display().to_string()])
        .output()
        .map_err(|e| {
            format!(
                "could not run sops: {e}\n\
                 Install sops (https://github.com/getsops/sops) and age."
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "sops failed to decrypt {}:\n{stderr}",
            path.display()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| format!("sops decrypted {} as non-UTF-8", path.display()))
}

/// Encrypted files start with a SOPS stanza or `ENC[` values. Plaintext example
/// files are parsed as-is so `migrate` and tests do not need the binary.
pub fn load_secrets_yaml(path: &Path) -> Result<String, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    if is_sops_encrypted(&raw) {
        decrypt_sops(path)
    } else {
        Ok(raw)
    }
}

pub fn is_sops_encrypted(yaml: &str) -> bool {
    yaml.contains("ENC[")
        || yaml
            .lines()
            .any(|l| l.trim() == "sops:" || l.starts_with("sops:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
github_repo = "acme/acme"

[production]
kubernetes_context = "prod"
monitoring_url = "https://monitoring.example.com"

[production.hosts]
api = "api.example.com"
app = "app.example.com"
www = "example.com"
admin = "admin.example.com"
"#;

    #[test]
    fn parses_app_config_and_fills_defaults() {
        let file = parse_deploy_file(CONFIG).unwrap();
        assert_eq!(file.github_repo, "acme/acme");
        let env = env(&file, "production").unwrap();
        assert_eq!(env.kubernetes_context, "prod");
        assert_eq!(env.namespace, "default");
        assert_eq!(env.hosts.api, "api.example.com");
        assert!(env.tls.enabled);
        assert_eq!(env.tls.issuer, "letsencrypt");
        assert!(env.workloads.admin);
        assert_eq!(env.workloads.api_replicas, 1);
        env.validate(Target::App).unwrap();
    }

    #[test]
    fn missing_env_names_the_section() {
        let file = parse_deploy_file(CONFIG).unwrap();
        let err = env(&file, "staging").unwrap_err();
        assert!(err.contains("[staging]"), "{err}");
    }

    #[test]
    fn www_host_is_required_only_when_www_is_on() {
        let mut file = parse_deploy_file(CONFIG).unwrap();
        let env = file.envs.get_mut("production").unwrap();
        env.hosts.www.clear();
        let err = env.validate(Target::App).unwrap_err();
        assert!(err.contains("hosts.www"), "{err}");
        env.workloads.www = false;
        env.validate(Target::App).unwrap();
    }

    #[test]
    fn monitoring_requires_scrape_target_when_prometheus_is_on() {
        let toml = r#"
github_repo = "acme/acme"
[production]
kubernetes_context = "mon"
[production.hosts]
monitoring = "monitoring.example.com"
"#;
        let file = parse_deploy_file(toml).unwrap();
        let env = env(&file, "production").unwrap();
        let err = env.validate(Target::Monitoring).unwrap_err();
        assert!(err.contains("scrape.target"), "{err}");
    }

    #[test]
    fn unknown_config_keys_are_rejected() {
        let err = parse_deploy_file(
            r#"
github_repo = "acme/acme"
[production]
kubernetes_context = "prod"
typo = true
"#,
        )
        .unwrap_err();
        assert!(err.contains("typo") || err.contains("unknown"), "{err}");
    }

    #[test]
    fn image_tag_strips_the_monitoring_prefix() {
        assert_eq!(image_tag("v1.2.3", Target::App), "v1.2.3");
        assert_eq!(image_tag("v1.2.3", Target::Monitoring), "v1.2.3");
        assert_eq!(image_tag("mon-v1.2.3", Target::Monitoring), "v1.2.3");
        assert_eq!(image_tag(" mon-v1.2.3 ", Target::Monitoring), "v1.2.3");
    }

    #[test]
    fn helm_shaped_secrets_are_rejected() {
        let old = "api:\n  imageTag: \"v1\"\n  database_url: postgres://x\n";
        assert!(looks_like_helm_values(old));
        let err = parse_app_secrets(old).unwrap_err();
        assert!(err.contains("migrate"), "{err}");
    }

    #[test]
    fn app_secrets_parse_and_default_smtp_port() {
        let yaml = r#"
registry:
  server: ghcr.io
  username: u
  password: p
api:
  database_url: postgres://u:p@h/db
  jwt_secret: s
  ingest_token: tok
"#;
        let s = parse_app_secrets(yaml).unwrap();
        assert_eq!(s.api.smtp_port, 587);
        assert_eq!(s.api.log_level, "info");
        assert_eq!(s.api.ingest_token, "tok");
    }

    #[test]
    fn leftover_error_reporting_block_is_helm_shaped() {
        let yaml = "api:\n  error_reporting:\n    ingest_token: x\n";
        assert!(looks_like_helm_values(yaml));
    }

    #[test]
    fn sops_detection() {
        assert!(is_sops_encrypted("api: ENC[AES256_GCM,data:abc]\n"));
        assert!(is_sops_encrypted("sops:\n  kms: []\n"));
        assert!(!is_sops_encrypted("api:\n  jwt_secret: plaintext\n"));
    }

    #[test]
    fn scaffold_templates_parse() {
        let app_toml = include_str!("../../templates/deploy/config.toml")
            .replace("{{github_repo}}", "acme/acme")
            .replace("{{kubernetes_context}}", "prod");
        let app = parse_deploy_file(&app_toml).unwrap();
        app.envs["production"].validate(Target::App).unwrap();

        let app_secrets = include_str!("../../templates/deploy/secrets.example.yaml")
            .replace("{{admin_password_hash}}", "$argon2id$x");
        parse_app_secrets(&app_secrets).unwrap();

        let mon_toml = include_str!("../../templates/deploy/monitoring/config.toml")
            .replace("{{github_repo}}", "acme/acme")
            .replace("{{monitoring_kubernetes_context}}", "mon");
        let mon = parse_deploy_file(&mon_toml).unwrap();
        mon.envs["production"].validate(Target::Monitoring).unwrap();

        let mon_secrets = include_str!("../../templates/deploy/monitoring/secrets.example.yaml")
            .replace("{{admin_password_hash}}", "$argon2id$x")
            .replace("{{ingest_token}}", "tok");
        parse_monitoring_secrets(&mon_secrets).unwrap();
    }

    #[test]
    fn layout_paths_do_not_overlap() {
        let app = Layout::for_target(Target::App);
        let mon = Layout::for_target(Target::Monitoring);
        assert_eq!(app.config_path().to_str().unwrap(), "deploy/config.toml");
        assert_eq!(
            mon.config_path().to_str().unwrap(),
            "monitoring/deploy/config.toml"
        );
        assert_eq!(app.legacy_chart_dir(), "chart");
        assert!(mon.legacy_chart_dir().starts_with("monitoring/"));
        assert_ne!(app.dir, mon.dir);
    }
}
