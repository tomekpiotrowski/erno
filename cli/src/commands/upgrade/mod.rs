mod plan;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::Args;

use crate::commands::dev::resolve_project_root;
use crate::commands::packages::run_prefixed;
use crate::ui;

pub use plan::{angular_majors, plan_upgrade, GitStatus, ProjectSnapshot, StepKind};

#[derive(Args, Debug, Default)]
pub struct UpgradeArgs {
    /// Print the plan and exit
    #[arg(long)]
    pub dry_run: bool,
    /// Run without prompting
    #[arg(long)]
    pub yes: bool,
    /// Allow a dirty git worktree
    #[arg(long)]
    pub force: bool,
}

pub async fn handle_upgrade(args: UpgradeArgs) -> ui::Cmd {
    let root = resolve_project_root(None).map_err(ui::Failure::from)?;
    let snap = snapshot(&root, args.force)?;
    let plan = plan_upgrade(&snap);

    ui::section(ui::icon::UPGRADE, "Upgrade plan");
    ui::blank();
    print_plan(&plan);

    if args.dry_run {
        if !plan.blocking.is_empty() {
            return Ok(());
        }
        if plan.is_current() {
            ui::blank();
            ui::finished(ui::icon::DONE, "Everything is current.");
        }
        return Ok(());
    }

    if !plan.blocking.is_empty() {
        return Err(ui::Failure::Message(plan.blocking.join("\n")));
    }
    if plan.is_current() {
        ui::blank();
        ui::finished(ui::icon::DONE, "Everything is current.");
        return Ok(());
    }

    let n = plan.steps.len();
    let proceed = args.yes || ui::confirm(&format!("Proceed with {n} updates?"), true);
    if !proceed {
        ui::warn("cancelled");
        return Ok(());
    }

    for step in &plan.steps {
        ui::section(ui::icon::PACKAGE, &step.label);
        if !execute_step(&root, step) {
            return Err(ui::Failure::Message(format!(
                "{} failed — earlier steps were applied; git is the undo",
                step.label
            )));
        }
        ui::ok(format!("{} {}", step.label, step.target));
    }

    if plan.steps.iter().any(|s| {
        matches!(
            s.kind,
            StepKind::ErnoCrate {
                spec: plan::CrateSpec::Git { .. }
            } | StepKind::ErnoCrate {
                spec: plan::CrateSpec::Version(_)
            }
        )
    }) {
        ui::detail("If the erno crate moved, run `cargo run -- db migrate up` in api/.");
    }
    let plural = if n == 1 { "update" } else { "updates" };
    ui::finished(ui::icon::DONE, format!("{n} {plural} applied"));
    Ok(())
}

fn print_plan(plan: &plan::UpgradePlan) {
    let mut rows = Vec::new();
    for b in &plan.blocking {
        rows.push(ui::Row::fail("blocked", b.clone()));
    }
    for step in &plan.steps {
        let mut row = ui::Row::ok(
            step.label.clone(),
            format!("{} → {}", step.current, step.target),
        );
        row.hint = Some(step.how.clone());
        rows.push(row);
    }
    if rows.is_empty() {
        rows.push(ui::Row::ok("project", "already on this CLI's targets"));
    }
    ui::print_rows(&rows);
}

fn snapshot(root: &Path, force: bool) -> Result<ProjectSnapshot, ui::Failure> {
    Ok(ProjectSnapshot {
        node: run_text("node", &["--version"]),
        git: inspect_git(root),
        force,
        app_package: read_if(root.join("app/package.json")),
        admin_package: read_if(root.join("admin/package.json")),
        api_cargo: read_if(root.join("api/Cargo.toml")),
    })
}

fn read_if(path: PathBuf) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn run_text(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}

fn inspect_git(root: &Path) -> GitStatus {
    let dir = root.to_str().unwrap_or(".");
    let inside = match Command::new("git")
        .args(["-C", dir, "rev-parse", "--is-inside-work-tree"])
        .output()
    {
        Err(_) => return GitStatus::GitMissing,
        Ok(o) => o,
    };
    if !inside.status.success() {
        return GitStatus::NotARepo;
    }
    match Command::new("git")
        .args(["-C", dir, "status", "--porcelain"])
        .output()
    {
        Ok(o) if o.status.success() && o.stdout.is_empty() => GitStatus::Clean,
        Ok(o) if o.status.success() => GitStatus::Dirty,
        _ => GitStatus::NotARepo,
    }
}

fn execute_step(root: &Path, step: &plan::UpgradeStep) -> bool {
    match &step.kind {
        StepKind::Angular { dir, from_major } => {
            let dir = root.join(dir);
            for major in angular_majors(*from_major) {
                if !ng_update(&dir, major) {
                    return false;
                }
            }
            true
        }
        StepKind::Ionic { dir, .. } => ionic_migrate(&root.join(dir)),
        StepKind::ErnoAngular { .. } => {
            let pkg = root.join("app/package.json");
            let url = crate::version::erno_angular_tarball_url();
            if !rewrite_json_dep(&pkg, "erno-angular", &url) {
                return false;
            }
            let dir = root.join("app");
            let mut cmd = crate::bun::install(&dir);
            apply_ci(&mut cmd);
            run_prefixed(&mut cmd, "app")
        }
        StepKind::ErnoCrate { spec } => match spec {
            plan::CrateSpec::Path(_) => {
                ui::warn("path dependency — update the erno checkout, then rebuild");
                true
            }
            plan::CrateSpec::Git { .. } | plan::CrateSpec::Version(_) => {
                let cargo = root.join("api/Cargo.toml");
                let Ok(src) = fs::read_to_string(&cargo) else {
                    return false;
                };
                let tag = crate::version::erno_tag();
                if fs::write(&cargo, plan::rewrite_erno_to_git_tag(&src, &tag)).is_err() {
                    return false;
                }
                let dir = root.join("api");
                let mut cmd = Command::new("cargo");
                cmd.args(["update", "-p", "erno"]).current_dir(&dir);
                apply_ci(&mut cmd);
                run_prefixed(&mut cmd, "api")
            }
        },
    }
}

fn ng_update(dir: &Path, major: u32) -> bool {
    if let Err(error) = crate::bun::configure_angular(dir) {
        ui::warn(format!("could not configure Angular for Bun: {error}"));
        return false;
    }
    let ng = local_ng(dir).unwrap_or_else(|| PathBuf::from("bun"));
    let mut cmd = Command::new(&ng);
    if ng.file_name().and_then(|s| s.to_str()) == Some("bun") {
        cmd.args(["x", "--package", "@angular/cli", "ng"]);
    }
    // The CLI already refused a dirty tree (or the user passed --force). Later
    // majors in this same run dirtied the tree; --allow-dirty is how we continue.
    cmd.args([
        "update",
        &format!("@angular/core@{major}"),
        &format!("@angular/cli@{major}"),
        "--allow-dirty",
    ]);
    cmd.current_dir(dir)
        .env("BUN_FEATURE_FLAG_DISABLE_STREAMING_INSTALL", "1");
    apply_ci(&mut cmd);
    run_prefixed(&mut cmd, "ng")
}

fn ionic_migrate(dir: &Path) -> bool {
    let mut cmd = Command::new("bun");
    // Same reason as ng update --allow-dirty: prior steps in this run write files.
    cmd.args(["x", "@ionic/migrate", "--force"])
        .env("BUN_FEATURE_FLAG_DISABLE_STREAMING_INSTALL", "1");
    cmd.current_dir(dir);
    apply_ci(&mut cmd);
    run_prefixed(&mut cmd, "ionic")
}

fn local_ng(dir: &Path) -> Option<PathBuf> {
    let local = dir.join("node_modules/.bin/ng");
    local.is_file().then_some(local)
}

fn apply_ci(cmd: &mut Command) {
    cmd.env("CI", "true")
        .env("NG_CLI_ANALYTICS", "false")
        .stdin(Stdio::null());
}

fn rewrite_json_dep(path: &Path, name: &str, value: &str) -> bool {
    let Ok(src) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(mut pkg) = serde_json::from_str::<serde_json::Value>(&src) else {
        return false;
    };
    pkg["dependencies"][name] = serde_json::Value::String(value.to_string());
    let Ok(out) = serde_json::to_string_pretty(&pkg) else {
        return false;
    };
    fs::write(path, out + "\n").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "erno-upgrade-git-{}-{}-{suffix}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(["-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(dir)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn inspect_git_clean_after_init() {
        let dir = temp_dir("clean");
        assert!(git(&dir, &["init", "--quiet"]));
        assert_eq!(inspect_git(&dir), GitStatus::Clean);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspect_git_dirty_with_untracked_file() {
        let dir = temp_dir("dirty");
        assert!(git(&dir, &["init", "--quiet"]));
        fs::write(dir.join("x"), "x").unwrap();
        assert_eq!(inspect_git(&dir), GitStatus::Dirty);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspect_git_not_a_repo() {
        let dir = temp_dir("norepo");
        assert_eq!(inspect_git(&dir), GitStatus::NotARepo);
        let _ = fs::remove_dir_all(&dir);
    }
}
