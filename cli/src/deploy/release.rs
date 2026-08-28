//! In-cluster revision record and the collector "this version is live" webhook.
//!
//! The Secret stores component *names* and image tags, never rendered manifests
//! — those contain database URLs. Rollback of bad code re-installs the previous
//! version with the current secrets file.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::apply::{apply_file_argv, run};
use super::render::Manifest;
use crate::ui;

pub fn secret_name(release: &str) -> String {
    format!("erno-release-{release}")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Revision {
    pub version: String,
    pub deployed_at: u64,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Component {
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ReleaseState {
    pub current: Option<Revision>,
    pub previous: Option<Revision>,
}

impl ReleaseState {
    pub fn names(&self) -> Vec<String> {
        self.current
            .as_ref()
            .map(|r| r.components.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default()
    }
}

pub fn revision_from(version: &str, manifests: &[Manifest]) -> Revision {
    Revision {
        version: version.to_string(),
        deployed_at: now_unix(),
        components: manifests
            .iter()
            .filter(|m| m.prune)
            .map(|m| Component {
                kind: m.kind.clone(),
                name: m.name.clone(),
                image: m
                    .doc
                    .pointer("/spec/template/spec/containers/0/image")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            })
            .collect(),
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn release_secret_yaml(
    release: &str,
    namespace: &str,
    state: &ReleaseState,
) -> Result<String, String> {
    let current = serde_json::to_string(&state.current).map_err(|e| e.to_string())?;
    let previous = serde_json::to_string(&state.previous).map_err(|e| e.to_string())?;
    let doc = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {
            "name": secret_name(release),
            "namespace": namespace,
            "labels": {
                "app.kubernetes.io/managed-by": "erno",
                "app.kubernetes.io/name": "release",
            },
        },
        "type": "Opaque",
        "stringData": {
            "current": current,
            "previous": previous,
        }
    });
    serde_yaml::to_string(&doc).map_err(|e| e.to_string())
}

pub fn write_release(
    context: &str,
    namespace: &str,
    release: &str,
    state: &ReleaseState,
) -> Result<(), String> {
    let yaml = release_secret_yaml(release, namespace, state)?;
    let out = run(&apply_file_argv(context, namespace), Some(&yaml))?;
    if out.status != 0 {
        return Err(format!(
            "could not record the release Secret:\n{}{}",
            out.stderr, out.stdout
        ));
    }
    Ok(())
}

pub fn read_release(
    context: &str,
    namespace: &str,
    release: &str,
) -> Result<Option<ReleaseState>, String> {
    let out = run(
        &super::apply::get_secret_argv(context, namespace, &secret_name(release)),
        None,
    )?;
    if out.status != 0 {
        if out.stderr.contains("NotFound") || out.stderr.contains("not found") {
            return Ok(None);
        }
        return Err(format!(
            "could not read {}:\n{}{}",
            secret_name(release),
            out.stderr,
            out.stdout
        ));
    }
    parse_release_secret(&out.stdout)
}

pub fn parse_release_secret(stdout: &str) -> Result<Option<ReleaseState>, String> {
    let stdout = stdout.trim();
    if stdout.is_empty() || stdout == "---" {
        return Ok(Some(ReleaseState::default()));
    }
    let mut parts = stdout.splitn(2, "\n---\n");
    let current_raw = parts.next().unwrap_or("").trim();
    let previous_raw = parts.next().unwrap_or("").trim();
    let current = parse_opt_revision(current_raw)?;
    let previous = parse_opt_revision(previous_raw)?;
    Ok(Some(ReleaseState { current, previous }))
}

fn parse_opt_revision(raw: &str) -> Result<Option<Revision>, String> {
    if raw.is_empty() || raw == "null" || raw == "<no value>" {
        return Ok(None);
    }
    serde_json::from_str(raw).map_err(|e| format!("invalid revision record: {e}"))
}

pub fn advance(state: Option<ReleaseState>, next: Revision) -> ReleaseState {
    let previous = state.and_then(|s| s.current);
    ReleaseState {
        current: Some(next),
        previous,
    }
}

/// Tell the collector that a version is now live. Never fatal: a deploy that
/// actually succeeded must not be reported as failed because monitoring was
/// unreachable.
pub async fn record_release_webhook(collector_url: &str, version: &str, env: &str) {
    if collector_url.trim().is_empty() {
        return;
    }
    let Ok(token) = std::env::var("ERNO_INGEST_TOKEN") else {
        ui::info("skipping the release webhook — ERNO_INGEST_TOKEN is not set");
        return;
    };
    let url = format!(
        "{}/api/collector/releases",
        collector_url.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "version": version,
        "environment": env,
        "commit_sha": commit_sha_for(version),
        "source": "cli",
    });
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    match client
        .post(&url)
        .header("X-Erno-Ingest-Key", token)
        .json(&body)
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            ui::ok(format!("recorded release {version} with the collector"));
        }
        Ok(res) => ui::warn(format!(
            "the collector rejected the release webhook ({})",
            res.status()
        )),
        Err(e) => ui::warn(format!("could not reach the collector: {e}")),
    }
}

fn commit_sha_for(version: &str) -> Option<String> {
    let described = std::process::Command::new("git")
        .args(["describe", "--exact-match", "--tags"])
        .output()
        .ok()?;
    if !described.status.success() {
        return None;
    }
    let tag = String::from_utf8(described.stdout).ok()?;
    if tag.trim() != version
        && tag.trim().trim_start_matches('v') != version.trim_start_matches('v')
    {
        return None;
    }
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    head.status
        .success()
        .then(|| String::from_utf8_lossy(&head.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advancing_a_release_keeps_the_previous_generation() {
        let first = Revision {
            version: "v1".into(),
            deployed_at: 1,
            components: vec![Component {
                kind: "Deployment".into(),
                name: "acme-api".into(),
                image: Some("ghcr.io/acme/api:v1".into()),
            }],
        };
        let second = Revision {
            version: "v2".into(),
            deployed_at: 2,
            components: vec![Component {
                kind: "Deployment".into(),
                name: "acme-api".into(),
                image: Some("ghcr.io/acme/api:v2".into()),
            }],
        };
        let state = advance(None, first.clone());
        assert_eq!(state.current.as_ref().unwrap().version, "v1");
        assert!(state.previous.is_none());
        let state = advance(Some(state), second);
        assert_eq!(state.current.as_ref().unwrap().version, "v2");
        assert_eq!(state.previous.as_ref().unwrap().version, "v1");
    }

    #[test]
    fn release_secret_is_not_on_the_instance_label() {
        let yaml = release_secret_yaml(
            "acme",
            "default",
            &ReleaseState {
                current: Some(Revision {
                    version: "v1".into(),
                    deployed_at: 0,
                    components: vec![],
                }),
                previous: None,
            },
        )
        .unwrap();
        assert!(yaml.contains("erno-release-acme"));
        assert!(yaml.contains("app.kubernetes.io/name: release"));
        assert!(!yaml.contains("app.kubernetes.io/instance"));
        assert!(yaml.contains("stringData"));
    }

    #[test]
    fn parse_secret_stdout_round_trips() {
        let rev = Revision {
            version: "v3".into(),
            deployed_at: 9,
            components: vec![],
        };
        let raw = format!("{}\n---\nnull", serde_json::to_string(&rev).unwrap());
        let state = parse_release_secret(&raw).unwrap().unwrap();
        assert_eq!(state.current.unwrap().version, "v3");
        assert!(state.previous.is_none());
    }
}
