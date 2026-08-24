//! kubectl server-side apply, wait, prune, and rollback helpers.
//!
//! The CLI shells out, matching every other external tool it drives. Argv
//! builders are the unit-tested surface; nothing here talks to a cluster
//! under `cargo test`.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::render::Manifest;

pub const APPLY_TIMEOUT: Duration = Duration::from_secs(300);

pub const PRUNE_ALLOWLIST: &[&str] = &[
    "core/v1/ConfigMap",
    "core/v1/Secret",
    "core/v1/Service",
    "core/v1/PersistentVolumeClaim",
    "apps/v1/Deployment",
    "networking.k8s.io/v1/Ingress",
];

pub fn instance_selector(release: &str) -> String {
    format!("app.kubernetes.io/managed-by=erno,app.kubernetes.io/instance={release}")
}

pub fn apply_argv(context: &str, namespace: &str, release: &str) -> Vec<String> {
    let mut args = vec![
        "apply".into(),
        "--server-side".into(),
        "--field-manager=erno".into(),
        "--force-conflicts".into(),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
        "--prune".into(),
        format!("-l={}", instance_selector(release)),
    ];
    for kind in PRUNE_ALLOWLIST {
        args.push(format!("--prune-allowlist={kind}"));
    }
    args.push("-f".into());
    args.push("-".into());
    args
}

pub fn diff_argv(context: &str, namespace: &str) -> Vec<String> {
    vec![
        "diff".into(),
        "--server-side".into(),
        "--field-manager=erno".into(),
        "--force-conflicts".into(),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
        "-f".into(),
        "-".into(),
    ]
}

pub fn rollout_status_argv(
    context: &str,
    namespace: &str,
    name: &str,
    timeout_secs: u64,
) -> Vec<String> {
    vec![
        "rollout".into(),
        "status".into(),
        format!("deployment/{name}"),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
        format!("--timeout={timeout_secs}s"),
    ]
}

pub fn rollout_undo_argv(context: &str, namespace: &str, name: &str) -> Vec<String> {
    vec![
        "rollout".into(),
        "undo".into(),
        format!("deployment/{name}"),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
    ]
}

pub fn delete_labeled_argv(context: &str, namespace: &str, release: &str) -> Vec<String> {
    vec![
        "delete".into(),
        "deploy,svc,ingress,secret,cm,pvc".into(),
        format!("-l={}", instance_selector(release)),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
        "--wait=true".into(),
        "--ignore-not-found=true".into(),
    ]
}

pub fn delete_resource_argv(context: &str, namespace: &str, kind: &str, name: &str) -> Vec<String> {
    vec![
        "delete".into(),
        kind.to_lowercase(),
        name.into(),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
        "--ignore-not-found=true".into(),
        "--wait=false".into(),
    ]
}

pub fn use_context_argv(context: &str) -> Vec<String> {
    vec!["config".into(), "use-context".into(), context.into()]
}

/// Cluster-wide apply of an upstream release manifest. No namespace, no prune:
/// the YAML owns its own namespaces and we must not delete unrelated objects.
pub fn apply_url_argv(context: &str, url: &str) -> Vec<String> {
    vec![
        "apply".into(),
        format!("--context={context}"),
        "-f".into(),
        url.into(),
    ]
}

pub fn get_crd_argv(context: &str, crd: &str) -> Vec<String> {
    vec![
        "get".into(),
        "crd".into(),
        crd.into(),
        format!("--context={context}"),
        "-o".into(),
        "name".into(),
    ]
}

pub fn get_namespaced_deploy_argv(context: &str, namespace: &str, name: &str) -> Vec<String> {
    vec![
        "get".into(),
        "deploy".into(),
        name.into(),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
        "-o".into(),
        "name".into(),
    ]
}

pub fn get_secret_argv(context: &str, namespace: &str, name: &str) -> Vec<String> {
    vec![
        "get".into(),
        "secret".into(),
        name.into(),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
        "-o".into(),
        "go-template={{index .data \"current\" | base64decode}}{{println}}---{{println}}{{index .data \"previous\" | base64decode}}".into(),
    ]
}

pub fn apply_file_argv(context: &str, namespace: &str) -> Vec<String> {
    vec![
        "apply".into(),
        "--server-side".into(),
        "--field-manager=erno".into(),
        "--force-conflicts".into(),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
        "-f".into(),
        "-".into(),
    ]
}

pub fn get_deployments_argv(context: &str, namespace: &str, release: &str) -> Vec<String> {
    vec![
        "get".into(),
        "deploy".into(),
        format!("-l={}", instance_selector(release)),
        format!("--context={context}"),
        format!("--namespace={namespace}"),
        "-o".into(),
        "jsonpath={range .items[*]}{.metadata.name}{'\\n'}{end}".into(),
    ]
}

#[derive(Debug)]
pub struct KubectlOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(args: &[String], stdin: Option<&str>) -> Result<KubectlOutput, String> {
    run_inherit(args, stdin, false)
}

pub fn run_inherit(
    args: &[String],
    stdin: Option<&str>,
    inherit: bool,
) -> Result<KubectlOutput, String> {
    let mut cmd = Command::new("kubectl");
    cmd.args(args);
    if inherit && stdin.is_none() {
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = cmd
            .status()
            .map_err(|e| format!("could not run kubectl: {e}"))?;
        if !status.success() {
            return Err(format!(
                "kubectl {} exited with status {status}",
                args.first().unwrap_or(&"kubectl".into())
            ));
        }
        return Ok(KubectlOutput {
            status: status.code().unwrap_or(1),
            stdout: String::new(),
            stderr: String::new(),
        });
    }
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not run kubectl: {e}"))?;
    if let Some(body) = stdin {
        let Some(mut pipe) = child.stdin.take() else {
            return Err("kubectl stdin pipe missing".into());
        };
        pipe.write_all(body.as_bytes())
            .map_err(|e| format!("could not write kubectl stdin: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("kubectl failed: {e}"))?;
    Ok(KubectlOutput {
        status: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

pub fn apply_and_prune(
    context: &str,
    namespace: &str,
    release: &str,
    yaml: &str,
) -> Result<(), String> {
    let out = run(&apply_argv(context, namespace, release), Some(yaml))?;
    if out.status != 0 {
        return Err(format!(
            "kubectl apply failed:\n{}{}",
            out.stderr, out.stdout
        ));
    }
    Ok(())
}

pub fn wait_deployments(
    context: &str,
    namespace: &str,
    names: &[String],
    total: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + total;
    for name in names {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!(
                "timed out waiting for deployments after {}s (next: {name})",
                total.as_secs()
            ));
        }
        let secs = left.as_secs().max(1);
        let out = run(&rollout_status_argv(context, namespace, name, secs), None)?;
        if out.status != 0 {
            return Err(format!(
                "deployment/{name} did not become ready:\n{}{}",
                out.stderr, out.stdout
            ));
        }
    }
    Ok(())
}

/// Roll back a failed apply. Deployments that already existed get `rollout undo`;
/// objects this revision added are deleted. A first install deletes everything
/// with the instance label (never ClusterIssuer — it is unlabelled and cluster
/// scoped).
pub fn rollback_failed_apply(
    context: &str,
    namespace: &str,
    release: &str,
    applied: &[Manifest],
    previous_names: &[String],
) -> Result<(), String> {
    if previous_names.is_empty() {
        let out = run(&delete_labeled_argv(context, namespace, release), None)?;
        if out.status != 0 {
            return Err(format!(
                "rollback (delete labeled) failed:\n{}{}",
                out.stderr, out.stdout
            ));
        }
        return Ok(());
    }
    let previous: std::collections::HashSet<&str> =
        previous_names.iter().map(String::as_str).collect();
    for m in applied {
        if !m.prune {
            continue;
        }
        let existed = previous.contains(m.name.as_str());
        if let Some(dep) = &m.deployment {
            if existed {
                let out = run(&rollout_undo_argv(context, namespace, dep), None)?;
                if out.status != 0 {
                    return Err(format!(
                        "rollout undo {dep} failed:\n{}{}",
                        out.stderr, out.stdout
                    ));
                }
                continue;
            }
        }
        if !existed {
            let out = run(
                &delete_resource_argv(context, namespace, &m.kind, &m.name),
                None,
            )?;
            if out.status != 0 {
                return Err(format!(
                    "delete {}/{} failed:\n{}{}",
                    m.kind, m.name, out.stderr, out.stdout
                ));
            }
        }
    }
    Ok(())
}

pub fn deployment_names(manifests: &[Manifest]) -> Vec<String> {
    manifests
        .iter()
        .filter_map(|m| m.deployment.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_argv_prunes_only_the_allowlist() {
        let args = apply_argv("prod", "default", "acme");
        assert!(args.contains(&"--server-side".into()));
        assert!(args.contains(&"--field-manager=erno".into()));
        assert!(args.contains(&"--force-conflicts".into()));
        assert!(args.contains(&"--prune".into()));
        assert!(args.contains(&"--context=prod".into()));
        assert!(args.contains(&"--namespace=default".into()));
        assert!(args
            .iter()
            .any(|a| a.contains("app.kubernetes.io/instance=acme")));
        assert!(args.iter().any(|a| a.contains("managed-by=erno")));
        for kind in PRUNE_ALLOWLIST {
            assert!(
                args.iter()
                    .any(|a| a == &format!("--prune-allowlist={kind}")),
                "missing {kind} in {args:?}"
            );
        }
        assert!(!args.iter().any(|a| a.contains("ClusterIssuer")));
        assert_eq!(args[args.len() - 2], "-f");
        assert_eq!(args[args.len() - 1], "-");
    }

    #[test]
    fn first_install_failure_deletes_by_label_not_cluster_issuer() {
        let args = delete_labeled_argv("prod", "ns", "acme");
        assert!(args.contains(&"deploy,svc,ingress,secret,cm,pvc".into()));
        assert!(!args
            .iter()
            .any(|a| a.to_lowercase().contains("clusterissuer")));
        assert!(args.iter().any(|a| a.contains("instance=acme")));
    }

    #[test]
    fn rollout_wait_uses_remaining_timeout() {
        let args = rollout_status_argv("c", "ns", "acme-api", 12);
        assert_eq!(args[0], "rollout");
        assert_eq!(args[1], "status");
        assert_eq!(args[2], "deployment/acme-api");
        assert_eq!(args[5], "--timeout=12s");
    }

    #[test]
    fn diff_is_server_side_without_prune() {
        let args = diff_argv("c", "ns");
        assert!(args.contains(&"diff".into()));
        assert!(args.contains(&"--server-side".into()));
        assert!(!args.iter().any(|a| a.contains("prune")));
    }
}
