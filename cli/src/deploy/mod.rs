//! CLI-owned Kubernetes deploy: render, apply, wait, prune, rollback.
//!
//! Cluster add-ons (ingress-nginx, cert-manager) are installed by
//! `erno deploy setup` from their upstream static YAML. Helm is not used.

mod addons;
mod apply;
mod config;
mod migrate;
mod project;
mod release;
mod render;

pub use addons::{handle_setup, IngressProvider};
pub use config::Layout;
pub use project::{read_github_repo, read_project_name, validate_project_root};

use apply::{
    apply_and_prune, deployment_names, diff_argv, get_deployments_argv, rollback_failed_apply, run,
    use_context_argv, wait_deployments, APPLY_TIMEOUT,
};
use config::{
    env, image_tag, load_secrets_yaml, parse_app_secrets, parse_deploy_file,
    parse_monitoring_secrets, DeployFile, EnvConfig,
};
use render::{
    encode_yaml, load_extra, render_app, render_monitoring, AppPlan, Manifest, MonitoringPlan,
};

use crate::ui;

/// Which deployment a `deploy` command acts on.
///
/// The two are independent releases in independent clusters — that separation
/// is the whole point of the monitoring split, so every path, name and context
/// is derived from here rather than threaded through as booleans.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, clap::ValueEnum)]
pub enum Target {
    #[default]
    App,
    Monitoring,
}

impl Target {
    pub fn label(self) -> &'static str {
        match self {
            Target::App => "application",
            Target::Monitoring => "monitoring",
        }
    }

    /// Which deployment this tree *is*.
    ///
    /// Not a flag any more. The collector has its own repository, laid out as
    /// an ordinary Erno application, so the only thing a `--target` could still
    /// have chosen is which chart to render — and the tree already says. A flag
    /// there is just a way to deploy the wrong chart into the wrong cluster.
    #[must_use]
    pub fn detect() -> Self {
        if project::is_collector_tree() {
            Self::Monitoring
        } else {
            Self::App
        }
    }
}

struct Prepared {
    target: Target,
    release: String,
    version: String,
    env_name: String,
    env: EnvConfig,
    manifests: Vec<Manifest>,
    yaml: String,
}

fn require_layout(target: Target) -> Result<Layout, String> {
    let layout = Layout::for_target(target);
    if layout.config_exists() {
        if Path::new(layout.legacy_chart_dir()).exists() {
            ui::warn(format!(
                "{} still exists next to {}",
                layout.legacy_chart_dir(),
                layout.config_path().display()
            ));
            ui::detail(format!(
                "Remove it after a successful install:\n  git rm -r {}",
                layout.legacy_chart_dir()
            ));
        }
        return Ok(layout);
    }
    if Path::new(layout.legacy_chart_dir()).exists() {
        return Err(format!(
            "{} is a Helm chart; this CLI no longer installs those\n\
             Run `erno deploy migrate` to convert it to {}.",
            layout.legacy_chart_dir(),
            layout.config_path().display()
        ));
    }
    Err(format!(
        "missing {}\n\
         Run `erno deploy init` first.",
        layout.config_path().display()
    ))
}

fn prepare(version: &str, env_name: &str, target: Target) -> Result<Prepared, String> {
    validate_project_root(target);
    let layout = require_layout(target)?;
    // The release is the project name for both targets. The collector used to
    // carry a `-monitoring` suffix because it was deployed out of the
    // application's tree and the two releases had to differ; it has its own
    // repository, and its own project name, now.
    let release = read_project_name();
    let file = load_deploy_file(&layout)?;
    let env = env(&file, env_name)?.clone();
    env.validate(target)?;
    let tag = image_tag(version);
    let secrets_path = layout.secrets_path(env_name);
    if !secrets_path.exists() {
        return Err(format!(
            "missing {}\n\
             Copy {} to that path, fill in values, and encrypt with SOPS.",
            secrets_path.display(),
            layout.secrets_example().display()
        ));
    }
    let secrets_yaml = load_secrets_yaml(&secrets_path)?;
    let (mut manifests, extra_env) = match target {
        Target::App => {
            let secrets = parse_app_secrets(&secrets_yaml)?;
            let extra_env = secrets.env.clone();
            let manifests = render_app(&AppPlan {
                release: &release,
                github_repo: &file.github_repo,
                version: &tag,
                env: &env,
                secrets: &secrets,
                include_www: env.workloads.www && project::www_present(),
            });
            (manifests, extra_env)
        }
        Target::Monitoring => {
            let secrets = parse_monitoring_secrets(&secrets_yaml)?;
            let extra_env = secrets.env.clone();
            let manifests = render_monitoring(&MonitoringPlan {
                release: &release,
                github_repo: &file.github_repo,
                version: &tag,
                env: &env,
                secrets: &secrets,
            });
            (manifests, extra_env)
        }
    };
    manifests.extend(load_extra(
        &layout.extra_dir(),
        &release,
        &tag,
        &env.namespace,
        &extra_env,
    )?);
    let yaml = encode_yaml(&manifests)?;
    Ok(Prepared {
        target,
        release,
        version: tag,
        env_name: env_name.to_string(),
        env,
        manifests,
        yaml,
    })
}

fn load_deploy_file(layout: &Layout) -> Result<DeployFile, String> {
    let path = layout.config_path();
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    parse_deploy_file(&raw)
}

fn switch_context(context: &str) -> Result<(), String> {
    ui::section(
        ui::icon::CLOUD,
        format!("Switching kubectl context to '{context}'"),
    );
    let out = run(&use_context_argv(context), None)?;
    if out.status != 0 {
        return Err(format!(
            "kubectl config use-context {context} failed:\n{}{}",
            out.stderr, out.stdout
        ));
    }
    Ok(())
}

pub async fn handle_install(version: &str, env_name: &str, target: Target) -> ui::Cmd {
    let prepared = prepare(version, env_name, target)?;
    switch_context(&prepared.env.kubernetes_context)?;
    addons::require_addons(&prepared.env.kubernetes_context, prepared.env.tls.enabled)?;

    ui::section(
        ui::icon::DEPLOY,
        format!(
            "Deploying {} {} to {}",
            prepared.release, prepared.version, prepared.env_name
        ),
    );
    if ui::verbose() {
        for m in &prepared.manifests {
            ui::detail(format!("{}/{}", m.kind, m.name));
        }
    }

    let previous = release::read_release(
        &prepared.env.kubernetes_context,
        &prepared.env.namespace,
        &prepared.release,
    )?;
    let previous_names = previous.as_ref().map(|s| s.names()).unwrap_or_default();

    apply_and_prune(
        &prepared.env.kubernetes_context,
        &prepared.env.namespace,
        &prepared.release,
        &prepared.yaml,
    )?;

    let wait = wait_deployments(
        &prepared.env.kubernetes_context,
        &prepared.env.namespace,
        &deployment_names(&prepared.manifests),
        APPLY_TIMEOUT,
    );
    if let Err(e) = wait {
        ui::warn("rollout did not become ready — rolling back");
        if let Err(rb) = rollback_failed_apply(
            &prepared.env.kubernetes_context,
            &prepared.env.namespace,
            &prepared.release,
            &prepared.manifests,
            &previous_names,
        ) {
            return Err(format!("{e}\nrollback also failed: {rb}").into());
        }
        return Err(format!("{e}\nrolled back to the previous revision").into());
    }

    let next = release::revision_from(&prepared.version, &prepared.manifests);
    let state = release::advance(previous, next);
    if let Err(e) = release::write_release(
        &prepared.env.kubernetes_context,
        &prepared.env.namespace,
        &prepared.release,
        &state,
    ) {
        ui::warn(format!(
            "deploy succeeded but the revision Secret was not updated: {e}"
        ));
    }

    ui::blank();
    ui::ok(format!(
        "Deployed {} {} to {}",
        prepared.release, prepared.version, prepared.env_name
    ));

    if prepared.target == Target::App {
        release::record_release_webhook(
            &prepared.env.monitoring_url,
            &prepared.version,
            &prepared.env_name,
        )
        .await;
    }
    Ok(())
}

pub async fn handle_diff(version: &str, env_name: &str, target: Target) -> ui::Cmd {
    let prepared = prepare(version, env_name, target)?;
    switch_context(&prepared.env.kubernetes_context)?;
    ui::section(
        ui::icon::DEPLOY,
        format!(
            "Diff {} {} on {}",
            prepared.release, prepared.version, prepared.env_name
        ),
    );
    let out = run(
        &diff_argv(&prepared.env.kubernetes_context, &prepared.env.namespace),
        Some(&prepared.yaml),
    )?;
    // kubectl diff: 0 = no change, 1 = there is a diff, other = error.
    match out.status {
        0 => ui::ok("cluster already matches"),
        1 => {
            ui::emit_block(ui::Stream::Out, &out.stdout);
            if !out.stderr.trim().is_empty() {
                ui::detail(out.stderr.trim());
            }
        }
        _ => {
            return Err(format!("kubectl diff failed:\n{}{}", out.stderr, out.stdout).into());
        }
    }
    Ok(())
}

pub async fn handle_status(env_name: &str, target: Target) -> ui::Cmd {
    validate_project_root(target);
    let layout = require_layout(target)?;
    // The release is the project name for both targets. The collector used to
    // carry a `-monitoring` suffix because it was deployed out of the
    // application's tree and the two releases had to differ; it has its own
    // repository, and its own project name, now.
    let release = read_project_name();
    let file = load_deploy_file(&layout)?;
    let env = env(&file, env_name)?;
    switch_context(&env.kubernetes_context)?;
    ui::section(
        ui::icon::DEPLOY,
        format!("{release} on {env_name} ({})", env.kubernetes_context),
    );
    match release::read_release(&env.kubernetes_context, &env.namespace, &release)? {
        Some(state) => match state.current {
            Some(cur) => {
                ui::ok(format!("current {}", cur.version));
                if let Some(prev) = state.previous {
                    ui::detail(format!("previous {}", prev.version));
                }
                for c in cur.components {
                    match c.image {
                        Some(img) => ui::detail(format!("{}/{}  {img}", c.kind, c.name)),
                        None => ui::detail(format!("{}/{}", c.kind, c.name)),
                    }
                }
            }
            None => ui::info("no current revision recorded"),
        },
        None => ui::info(format!(
            "no {} Secret — this cluster has not been installed by this CLI",
            release::secret_name(&release)
        )),
    }
    let live = run(
        &get_deployments_argv(&env.kubernetes_context, &env.namespace, &release),
        None,
    )?;
    if live.status == 0 && !live.stdout.trim().is_empty() {
        ui::blank();
        ui::info("live deployments");
        for line in live.stdout.lines().filter(|l| !l.is_empty()) {
            ui::detail(line);
        }
    }
    Ok(())
}

pub async fn handle_rollback(env_name: &str, target: Target) -> ui::Cmd {
    validate_project_root(target);
    let layout = require_layout(target)?;
    // The release is the project name for both targets. The collector used to
    // carry a `-monitoring` suffix because it was deployed out of the
    // application's tree and the two releases had to differ; it has its own
    // repository, and its own project name, now.
    let release = read_project_name();
    let file = load_deploy_file(&layout)?;
    let env = env(&file, env_name)?;
    switch_context(&env.kubernetes_context)?;
    let state = release::read_release(&env.kubernetes_context, &env.namespace, &release)?
        .ok_or_else(|| format!("no revision Secret for {release} — nothing to roll back"))?;
    let Some(previous) = state.previous else {
        return Err("no previous revision — this is the first install".into());
    };
    ui::section(
        ui::icon::DEPLOY,
        format!("Rolling {release} back to {}", previous.version),
    );
    handle_install(&previous.version, env_name, target).await
}

pub fn handle_migrate(target: Target) -> ui::Cmd {
    validate_project_root(target);
    let github_repo = {
        let layout = Layout::for_target(target);
        if layout.config_exists() {
            load_deploy_file(&layout)
                .map(|f| f.github_repo)
                .unwrap_or_else(|_| read_github_repo())
        } else {
            read_github_repo()
        }
    };
    ui::section(
        ui::icon::DEPLOY,
        format!("Migrating {} off Helm", target.label()),
    );
    let notes = migrate::migrate(target, &github_repo)?;
    for note in notes {
        if note.starts_with("left ") || note.starts_with("  git") {
            ui::detail(note);
        } else {
            ui::ok(note);
        }
    }
    ui::blank();
    ui::ok("migrate complete");
    ui::detail(
        "Install the version that is already live before changing images:\n\
         erno deploy install <current-tag> --env production",
    );
    Ok(())
}

use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_targets_deploy_from_the_same_directory() {
        // They are never in one tree any more: an application repository holds
        // the app chart, the erno-monitoring repository holds the collector's.
        assert_eq!(
            Layout::for_target(Target::App).dir,
            Layout::for_target(Target::Monitoring).dir
        );
    }
}
