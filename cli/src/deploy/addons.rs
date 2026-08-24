//! Cluster add-ons Erno Ingress + TLS need: cert-manager and ingress-nginx.
//!
//! Both projects publish a static release YAML. `erno deploy setup` applies
//! those with kubectl — no Helm. Versions are pinned to this CLI generation.
//!
//! ingress-nginx itself is retired (no releases after March 2026). We still
//! install the last static manifest because Erno Ingress objects use class
//! `nginx`. Replacing the controller is a later migration, not a reason to
//! keep Helm around.

use super::apply::{
    apply_url_argv, get_crd_argv, get_namespaced_deploy_argv, rollout_status_argv, run,
    run_inherit, APPLY_TIMEOUT,
};
use super::{require_layout, switch_context, Target};
use crate::ui;

/// Pinned to a current supported cert-manager release. Bump here, not in apps.
pub const CERT_MANAGER_VERSION: &str = "v1.21.1";
pub const CERT_MANAGER_CRD: &str = "certificates.cert-manager.io";
pub const CERT_MANAGER_NS: &str = "cert-manager";

pub fn cert_manager_manifest() -> String {
    format!(
        "https://github.com/cert-manager/cert-manager/releases/download/{CERT_MANAGER_VERSION}/cert-manager.yaml"
    )
}

/// Last ingress-nginx controller with a static cloud/kind/baremetal YAML.
pub const INGRESS_NGINX_VERSION: &str = "controller-v1.13.2";
pub const INGRESS_NGINX_NS: &str = "ingress-nginx";
pub const INGRESS_NGINX_DEPLOY: &str = "ingress-nginx-controller";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum IngressProvider {
    /// Service type LoadBalancer — the production default.
    #[default]
    Cloud,
    Kind,
    Baremetal,
}

impl IngressProvider {
    pub fn from_config(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "cloud" => Ok(Self::Cloud),
            "kind" => Ok(Self::Kind),
            "baremetal" | "bare-metal" => Ok(Self::Baremetal),
            other => Err(format!(
                "unknown ingress_provider {other:?} — use cloud, kind, or baremetal"
            )),
        }
    }

    pub fn manifest_url(self) -> String {
        let provider = match self {
            Self::Cloud => "cloud",
            Self::Kind => "kind",
            Self::Baremetal => "baremetal",
        };
        format!(
            "https://raw.githubusercontent.com/kubernetes/ingress-nginx/{INGRESS_NGINX_VERSION}/deploy/static/provider/{provider}/deploy.yaml"
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::Cloud => "cloud (LoadBalancer)",
            Self::Kind => "kind",
            Self::Baremetal => "baremetal",
        }
    }
}

pub fn cert_manager_present(context: &str) -> Result<bool, String> {
    let out = run(&get_crd_argv(context, CERT_MANAGER_CRD), None)?;
    Ok(out.status == 0)
}

pub fn ingress_nginx_present(context: &str) -> Result<bool, String> {
    let out = run(
        &get_namespaced_deploy_argv(context, INGRESS_NGINX_NS, INGRESS_NGINX_DEPLOY),
        None,
    )?;
    Ok(out.status == 0)
}

/// Fail `install` before rendering if the cluster cannot serve Ingress/TLS.
pub fn require_addons(context: &str, tls: bool) -> Result<(), String> {
    let mut missing = Vec::new();
    if !ingress_nginx_present(context)? {
        missing.push("ingress-nginx");
    }
    if tls && !cert_manager_present(context)? {
        missing.push("cert-manager");
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "cluster is missing {} — Ingress/TLS will not work\n\
         Run `erno deploy setup` once per cluster (uses kubectl apply of the\n\
         upstream release YAML; Helm is not involved).",
        missing.join(" and ")
    ))
}

pub fn handle_setup(
    env_name: &str,
    target: Target,
    upgrade: bool,
    provider_flag: Option<IngressProvider>,
) -> ui::Cmd {
    super::project::validate_project_root(target);
    let layout = require_layout(target)?;
    let file = super::load_deploy_file(&layout)?;
    let env = super::env(&file, env_name)?.clone();
    let provider = match provider_flag {
        Some(p) => p,
        None => IngressProvider::from_config(&env.ingress_provider)?,
    };
    switch_context(&env.kubernetes_context)?;

    ui::section(
        ui::icon::DEPLOY,
        format!(
            "Setting up cluster add-ons on {} ({})",
            env.kubernetes_context,
            provider.label()
        ),
    );
    ui::detail(format!(
        "cert-manager {CERT_MANAGER_VERSION}\n\
         ingress-nginx {INGRESS_NGINX_VERSION}"
    ));

    apply_cert_manager(&env.kubernetes_context, upgrade)?;
    apply_ingress_nginx(&env.kubernetes_context, provider, upgrade)?;

    ui::blank();
    ui::ok("cluster add-ons are ready");
    if provider == IngressProvider::Cloud {
        ui::detail(
            "Point DNS at the ingress-nginx LoadBalancer IP:\n\
             kubectl get svc -n ingress-nginx ingress-nginx-controller",
        );
    }
    Ok(())
}

fn apply_cert_manager(context: &str, upgrade: bool) -> Result<(), String> {
    if cert_manager_present(context)? && !upgrade {
        ui::ok(format!(
            "cert-manager already installed (CRD {CERT_MANAGER_CRD})"
        ));
        return Ok(());
    }
    ui::info(format!("applying cert-manager {CERT_MANAGER_VERSION}"));
    apply_manifest(context, &cert_manager_manifest())?;
    wait_named(
        context,
        CERT_MANAGER_NS,
        &[
            "cert-manager",
            "cert-manager-cainjector",
            "cert-manager-webhook",
        ],
    )?;
    ui::ok("cert-manager ready");
    Ok(())
}

fn apply_ingress_nginx(
    context: &str,
    provider: IngressProvider,
    upgrade: bool,
) -> Result<(), String> {
    if ingress_nginx_present(context)? && !upgrade {
        ui::ok("ingress-nginx already installed");
        return Ok(());
    }
    let url = provider.manifest_url();
    ui::info(format!(
        "applying ingress-nginx {INGRESS_NGINX_VERSION} ({})",
        provider.label()
    ));
    apply_manifest(context, &url)?;
    wait_named(context, INGRESS_NGINX_NS, &[INGRESS_NGINX_DEPLOY])?;
    ui::ok("ingress-nginx ready");
    Ok(())
}

fn apply_manifest(context: &str, url: &str) -> Result<(), String> {
    let args = apply_url_argv(context, url);
    if ui::verbose() {
        ui::detail(format!("kubectl {}", args.join(" ")));
    }
    // Inherit kubectl's apply output — these manifests are noisy and the
    // operator should see which objects were created.
    run_inherit(&args, None, true)?;
    Ok(())
}

fn wait_named(context: &str, namespace: &str, names: &[&str]) -> Result<(), String> {
    let deadline = std::time::Instant::now() + APPLY_TIMEOUT;
    for name in names {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(format!(
                "timed out waiting for {namespace}/{name} after {}s",
                APPLY_TIMEOUT.as_secs()
            ));
        }
        let out = run(
            &rollout_status_argv(context, namespace, name, left.as_secs().max(1)),
            None,
        )?;
        if out.status != 0 {
            return Err(format!(
                "deployment/{name} in {namespace} did not become ready:\n{}{}",
                out.stderr, out.stdout
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::apply::apply_url_argv;

    #[test]
    fn cert_manager_url_is_the_official_release_manifest() {
        let url = cert_manager_manifest();
        assert!(url.contains(CERT_MANAGER_VERSION));
        assert!(url.ends_with("/cert-manager.yaml"));
        assert!(url.starts_with("https://github.com/cert-manager/"));
    }

    #[test]
    fn ingress_provider_urls_share_the_pinned_tag() {
        for p in [
            IngressProvider::Cloud,
            IngressProvider::Kind,
            IngressProvider::Baremetal,
        ] {
            let url = p.manifest_url();
            assert!(url.contains(INGRESS_NGINX_VERSION), "{url}");
            assert!(url.ends_with("/deploy.yaml"), "{url}");
        }
        assert!(IngressProvider::Cloud.manifest_url().contains("/cloud/"));
        assert!(IngressProvider::Kind.manifest_url().contains("/kind/"));
        assert!(IngressProvider::Baremetal
            .manifest_url()
            .contains("/baremetal/"));
    }

    #[test]
    fn apply_url_does_not_namespace_or_prune() {
        let args = apply_url_argv("prod", &cert_manager_manifest());
        assert!(args.contains(&"apply".into()));
        assert!(args.contains(&"--context=prod".into()));
        assert!(!args.iter().any(|a| a.starts_with("--namespace")));
        assert!(!args.iter().any(|a| a.contains("prune")));
        assert!(!args.iter().any(|a| a.contains("field-manager")));
    }

    #[test]
    fn ingress_provider_parses_config_aliases() {
        assert_eq!(
            IngressProvider::from_config("").unwrap(),
            IngressProvider::Cloud
        );
        assert_eq!(
            IngressProvider::from_config("CLOUD").unwrap(),
            IngressProvider::Cloud
        );
        assert_eq!(
            IngressProvider::from_config("kind").unwrap(),
            IngressProvider::Kind
        );
        assert_eq!(
            IngressProvider::from_config("bare-metal").unwrap(),
            IngressProvider::Baremetal
        );
        assert!(IngressProvider::from_config("istio").is_err());
    }
}
