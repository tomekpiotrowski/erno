//! Convert a Helm `chart/` tree into `deploy/`.
//!
//! Does not delete `chart/`. Helm template customizations are not compiled;
//! leftover `{{ .Values` is a warning, not an automatic `extra/` file.

use std::path::Path;

use super::config::Layout;
use super::Target;
use crate::ui;

pub fn migrate(target: Target, github_repo: &str) -> Result<Vec<String>, String> {
    let layout = Layout::for_target(target);
    let chart = Path::new(layout.legacy_chart_dir());
    if !chart.exists() {
        return Err(format!(
            "nothing to migrate — {} is missing\n\
             Run `erno deploy init{}` for a new layout.",
            chart.display(),
            match target {
                Target::App => "",
                Target::Monitoring => " --target monitoring",
            }
        ));
    }
    if layout.config_exists() {
        return Err(format!(
            "{} already exists — already migrated",
            layout.config_path().display()
        ));
    }

    let deploy_toml = read_optional(&chart.join("deploy.toml"))?;
    let values = read_optional(&chart.join("values.yaml"))?;
    let example = read_optional(&chart.join("secrets.example.yaml"))?;
    let sops = read_optional(&chart.join(".sops.yaml"))?;

    let config = convert_config(
        target,
        github_repo,
        deploy_toml.as_deref().unwrap_or(""),
        values.as_deref().unwrap_or(""),
        example.as_deref().unwrap_or(""),
    )?;

    std::fs::create_dir_all(layout.dir)
        .map_err(|e| format!("could not create {}: {e}", layout.dir))?;
    write(layout.config_path(), &config)?;
    if let Some(sops) = sops {
        write(layout.sops_path(), &sops)?;
    }
    if let Some(example) = example {
        write(
            layout.secrets_example(),
            &convert_secrets(target, &example)?,
        )?;
    }

    let mut converted_env = Vec::new();
    if let Ok(entries) = std::fs::read_dir(chart) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(env) = name
                .strip_prefix("secrets.")
                .and_then(|s| s.strip_suffix(".yaml"))
            else {
                continue;
            };
            if env == "example" {
                continue;
            }
            let path = entry.path();
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("could not read {}: {e}", path.display()))?;
            if super::config::is_sops_encrypted(&raw) {
                ui::warn(format!(
                    "{} is SOPS-encrypted — decrypt, convert, re-encrypt",
                    path.display()
                ));
                ui::detail(format!(
                    "sops -d {p} > {dest}\n\
                     # drop imageTag, ingress, api_url, collector_url; keep tokens\n\
                     sops -e -i {dest}",
                    p = path.display(),
                    dest = layout.secrets_path(env).display()
                ));
                continue;
            }
            write(layout.secrets_path(env), &convert_secrets(target, &raw)?)?;
            converted_env.push(env.to_string());
        }
    }

    let templates = chart.join("templates");
    if templates.exists() && helm_syntax_in(&templates) {
        ui::warn(format!(
            "{} still contains Helm templates",
            templates.display()
        ));
        ui::detail(
            "They are not evaluated. If you customized them, rewrite the result as\n\
             raw YAML in deploy/extra/ ({{release}}, {{version}}, {{namespace}} only).",
        );
    }

    let mut notes = vec![
        format!("wrote {}", layout.config_path().display()),
        format!(
            "left {} in place — remove it after the next successful install:",
            chart.display()
        ),
        format!("  git rm -r {}", chart.display()),
    ];
    if !converted_env.is_empty() {
        notes.push(format!(
            "converted plaintext secrets for: {}",
            converted_env.join(", ")
        ));
    }
    Ok(notes)
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("could not read {}: {e}", path.display())),
    }
}

fn write(path: impl AsRef<Path>, content: &str) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, content).map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn helm_syntax_in(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            return false;
        };
        text.contains("{{ .") || text.contains("{{-")
    })
}

pub fn convert_config(
    target: Target,
    github_repo: &str,
    deploy_toml: &str,
    values: &str,
    secrets_example: &str,
) -> Result<String, String> {
    let context = toml_value(deploy_toml, "kubernetes_context").unwrap_or_default();
    let monitoring_url = toml_value(deploy_toml, "monitoring_url").unwrap_or_default();
    if context.is_empty() {
        return Err("chart/deploy.toml has no kubernetes_context".into());
    }

    let mut out = format!("github_repo = \"{github_repo}\"\n\n[production]\nkubernetes_context = \"{context}\"\nnamespace = \"default\"\n");
    match target {
        Target::App => {
            out.push_str(&format!("monitoring_url = \"{monitoring_url}\"\n"));
            let api = yaml_str(secrets_example, &["ingress", "api", "host"])
                .or_else(|| yaml_str(secrets_example, &["api", "api_url"]).and_then(host_from_url))
                .unwrap_or_else(|| "api.example.com".into());
            let app = yaml_str(secrets_example, &["ingress", "app", "host"])
                .unwrap_or_else(|| "app.example.com".into());
            let www = yaml_str(secrets_example, &["ingress", "www", "host"])
                .unwrap_or_else(|| "example.com".into());
            let admin = yaml_str(secrets_example, &["ingress", "admin", "host"])
                .unwrap_or_else(|| "admin.example.com".into());
            let tls = yaml_bool(secrets_example, &["ingress", "tls"]).unwrap_or(true);
            let issuer = yaml_str(secrets_example, &["ingress", "issuer"])
                .unwrap_or_else(|| "letsencrypt".into());
            let email = yaml_str(secrets_example, &["ingress", "email"]).unwrap_or_default();
            let admin_on = yaml_bool(values, &["admin", "enabled"]).unwrap_or(true);
            out.push_str(&format!(
                "\n[production.hosts]\napi = \"{api}\"\napp = \"{app}\"\nwww = \"{www}\"\nadmin = \"{admin}\"\n"
            ));
            out.push_str(&format!(
                "\n[production.tls]\nenabled = {tls}\nissuer = \"{issuer}\"\nemail = \"{email}\"\n"
            ));
            out.push_str(&format!(
                "\n[production.workloads]\nadmin = {admin_on}\nwww = true\napi_replicas = {}\napp_replicas = {}\nwww_replicas = {}\nadmin_replicas = {}\n",
                yaml_i32(values, &["api", "replicas"]).unwrap_or(1),
                yaml_i32(values, &["app", "replicas"]).unwrap_or(1),
                yaml_i32(values, &["www", "replicas"]).unwrap_or(1),
                yaml_i32(values, &["admin", "replicas"]).unwrap_or(1),
            ));
        }
        Target::Monitoring => {
            let host = yaml_str(secrets_example, &["ingress", "monitoring", "host"])
                .unwrap_or_else(|| "monitoring.example.com".into());
            let tls = yaml_bool(secrets_example, &["ingress", "tls"]).unwrap_or(true);
            let issuer = yaml_str(secrets_example, &["ingress", "issuer"])
                .unwrap_or_else(|| "letsencrypt".into());
            let email = yaml_str(secrets_example, &["ingress", "email"]).unwrap_or_default();
            let target_host = yaml_str(secrets_example, &["api", "target"])
                .or_else(|| yaml_str(values, &["api", "target"]))
                .unwrap_or_else(|| "api.example.com:443".into());
            let scheme = yaml_str(secrets_example, &["api", "scheme"])
                .or_else(|| yaml_str(values, &["api", "scheme"]))
                .unwrap_or_else(|| "https".into());
            out.push_str(&format!("\n[production.hosts]\nmonitoring = \"{host}\"\n"));
            out.push_str(&format!(
                "\n[production.tls]\nenabled = {tls}\nissuer = \"{issuer}\"\nemail = \"{email}\"\n"
            ));
            out.push_str(&format!(
                "\n[production.scrape]\ntarget = \"{target_host}\"\nscheme = \"{scheme}\"\n"
            ));
            let rps = yaml_i32(values, &["ingress", "rateLimitRps"]).unwrap_or(20);
            out.push_str(&format!("\n[production.ingress]\nrate_limit_rps = {rps}\n"));
            let enabled = yaml_bool(values, &["prometheus", "enabled"]).unwrap_or(true);
            let image = yaml_str(values, &["prometheus", "image"])
                .unwrap_or_else(|| super::config::DEFAULT_PROMETHEUS_IMAGE.into());
            let retention =
                yaml_str(values, &["prometheus", "retention"]).unwrap_or_else(|| "90d".into());
            let storage =
                yaml_str(values, &["prometheus", "storage"]).unwrap_or_else(|| "10Gi".into());
            out.push_str(&format!(
                "\n[production.prometheus]\nenabled = {enabled}\nimage = \"{image}\"\nretention = \"{retention}\"\nstorage = \"{storage}\"\n"
            ));
        }
    }
    Ok(out)
}

pub fn convert_secrets(target: Target, yaml: &str) -> Result<String, String> {
    let mut v: serde_yaml::Value =
        serde_yaml::from_str(yaml).map_err(|e| format!("could not parse secrets YAML: {e}"))?;
    let map = v
        .as_mapping_mut()
        .ok_or_else(|| "secrets file is not a YAML mapping".to_string())?;
    map.remove(serde_yaml::Value::from("ingress"));
    match target {
        Target::App => {
            map.remove(serde_yaml::Value::from("app"));
            map.remove(serde_yaml::Value::from("www"));
            if let Some(api) = map.get_mut(serde_yaml::Value::from("api")) {
                if let Some(api) = api.as_mapping_mut() {
                    api.remove(serde_yaml::Value::from("imageTag"));
                    api.remove(serde_yaml::Value::from("api_url"));
                    let ingest = api
                        .get(serde_yaml::Value::from("error_reporting"))
                        .and_then(|er| er.get("ingest_token"))
                        .cloned();
                    api.remove(serde_yaml::Value::from("error_reporting"));
                    if let Some(ingest) = ingest {
                        api.insert(serde_yaml::Value::from("ingest_token"), ingest);
                    }
                }
            }
        }
        Target::Monitoring => {
            map.remove(serde_yaml::Value::from("console"));
            if let Some(c) = map.get_mut(serde_yaml::Value::from("collector")) {
                if let Some(c) = c.as_mapping_mut() {
                    c.remove(serde_yaml::Value::from("imageTag"));
                    c.remove(serde_yaml::Value::from("api_url"));
                }
            }
            if let Some(api) = map.get_mut(serde_yaml::Value::from("api")) {
                if let Some(api) = api.as_mapping_mut() {
                    api.remove(serde_yaml::Value::from("target"));
                    api.remove(serde_yaml::Value::from("scheme"));
                }
            }
        }
    }
    serde_yaml::to_string(&v).map_err(|e| e.to_string())
}

fn toml_value(src: &str, key: &str) -> Option<String> {
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix(key) else {
            continue;
        };
        let Some(val) = rest.trim().strip_prefix('=') else {
            continue;
        };
        let val = val.trim().trim_matches('"');
        return Some(val.to_string());
    }
    None
}

fn yaml_get(src: &str, path: &[&str]) -> Option<serde_yaml::Value> {
    let v: serde_yaml::Value = serde_yaml::from_str(src).ok()?;
    let mut cur = &v;
    for key in path {
        cur = cur.get(*key)?;
    }
    Some(cur.clone())
}

fn yaml_str(src: &str, path: &[&str]) -> Option<String> {
    match yaml_get(src, path)? {
        serde_yaml::Value::String(s) => Some(s),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
    .filter(|s| !s.is_empty())
}

fn yaml_bool(src: &str, path: &[&str]) -> Option<bool> {
    yaml_get(src, path)?.as_bool()
}

fn yaml_i32(src: &str, path: &[&str]) -> Option<i32> {
    yaml_get(src, path)?.as_i64().map(|n| n as i32)
}

fn host_from_url(url: String) -> Option<String> {
    url.split("://")
        .nth(1)
        .map(|s| s.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_DEPLOY: &str = r#"
[production]
kubernetes_context = "prod-ctx"
monitoring_url = "https://monitoring.example.com"
"#;

    const OLD_VALUES: &str = r#"
api:
  replicas: 2
app:
  replicas: 1
www:
  replicas: 1
admin:
  enabled: true
  replicas: 1
"#;

    const OLD_SECRETS: &str = r#"
registry:
  server: ghcr.io
  username: ""
  password: ""
api:
  imageTag: "v1.0.0"
  database_url: "postgres://u:p@h/db"
  jwt_secret: "s"
  api_url: "https://api.example.com"
  admin_password_hash: "hash"
  metrics_auth_token: ""
  smtp_host: ""
  smtp_port: 587
  smtp_username: ""
  smtp_password: ""
  smtp_from: ""
  log_level: "info"
  error_reporting:
    collector_url: "https://monitoring.example.com"
    ingest_token: "tok"
app:
  imageTag: ""
  api_url: "https://api.example.com"
www:
  imageTag: ""
  app_url: "https://app.example.com"
ingress:
  api:
    host: api.example.com
  app:
    host: app.example.com
  www:
    host: example.com
  admin:
    host: admin.example.com
  tls: true
  issuer: letsencrypt
  email: ops@example.com
"#;

    #[test]
    fn app_config_takes_hosts_from_secrets_example() {
        let cfg = convert_config(
            Target::App,
            "acme/acme",
            OLD_DEPLOY,
            OLD_VALUES,
            OLD_SECRETS,
        )
        .unwrap();
        assert!(cfg.contains("github_repo = \"acme/acme\""));
        assert!(cfg.contains("kubernetes_context = \"prod-ctx\""));
        assert!(cfg.contains("monitoring_url = \"https://monitoring.example.com\""));
        assert!(cfg.contains("api = \"api.example.com\""));
        assert!(cfg.contains("email = \"ops@example.com\""));
        assert!(cfg.contains("api_replicas = 2"));
        crate::deploy::config::parse_deploy_file(&cfg)
            .unwrap()
            .envs
            .get("production")
            .unwrap()
            .validate(Target::App)
            .unwrap();
    }

    #[test]
    fn app_secrets_drop_helm_keys_and_flatten_ingest_token() {
        let new = convert_secrets(Target::App, OLD_SECRETS).unwrap();
        assert!(!new.contains("imageTag"));
        assert!(!new.contains("ingress:"));
        assert!(!new.contains("collector_url"));
        assert!(!new.contains("error_reporting"));
        assert!(!new.contains("api_url"));
        assert!(new.contains("ingest_token: tok") || new.contains("ingest_token: \"tok\""));
        crate::deploy::config::parse_app_secrets(&new).unwrap();
    }

    #[test]
    fn monitoring_secrets_drop_image_tags_and_scrape_host() {
        let old = r#"
registry:
  server: ghcr.io
collector:
  imageTag: v0.1.0
  database_url: postgres://x
  jwt_secret: s
  api_url: https://monitoring.example.com
  server_token: t
console:
  imageTag: v0.1.0
api:
  target: "api.example.com:443"
  scheme: https
  metrics_auth_token: m
ingress:
  monitoring:
    host: monitoring.example.com
"#;
        let new = convert_secrets(Target::Monitoring, old).unwrap();
        assert!(!new.contains("imageTag"));
        assert!(!new.contains("api_url"));
        assert!(!new.contains("ingress:"));
        assert!(!new.contains("target:"));
        crate::deploy::config::parse_monitoring_secrets(&new).unwrap();
    }
}
