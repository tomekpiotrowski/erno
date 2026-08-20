mod plan;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::Args;

use crate::commands::dev::resolve_project_root;
use crate::commands::packages::run_prefixed;
use crate::ui;

pub use plan::{angular_majors, plan_upgrade, ProjectSnapshot, StepKind, TARGET_ERNO_ANGULAR};

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

    ui::section("Upgrade plan");
    ui::blank();
    print_plan(&plan);

    if !plan.blocking.is_empty() {
        return Err(ui::Failure::Message(plan.blocking.join("\n")));
    }
    if plan.is_current() {
        ui::blank();
        ui::ok("everything current");
        return Ok(());
    }
    if args.dry_run {
        return Ok(());
    }

    let n = plan.steps.len();
    let proceed = args.yes || ui::confirm(&format!("Proceed with {n} updates?"), true);
    if !proceed {
        ui::warn("cancelled");
        return Ok(());
    }

    for step in &plan.steps {
        ui::section(&step.label);
        if !execute_step(&root, step, args.force) {
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
                spec: plan::CrateSpec::Git
            } | StepKind::ErnoCrate {
                spec: plan::CrateSpec::Version(_)
            }
        )
    }) {
        ui::detail("If the erno crate moved, run `cargo run -- db migrate up` in api/.");
    }
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
        git_clean: git_clean(root),
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

fn git_clean(root: &Path) -> bool {
    Command::new("git")
        .args(["-C", root.to_str().unwrap_or("."), "status", "--porcelain"])
        .output()
        .ok()
        .map(|o| o.status.success() && o.stdout.is_empty())
        .unwrap_or(false)
}

fn execute_step(root: &Path, step: &plan::UpgradeStep, force: bool) -> bool {
    match &step.kind {
        StepKind::Angular { dir, from_major } => {
            let dir = root.join(dir);
            for major in angular_majors(*from_major) {
                if !ng_update(&dir, major, force) {
                    return false;
                }
            }
            true
        }
        StepKind::Ionic { dir, .. } => ionic_migrate(&root.join(dir), force),
        StepKind::ErnoAngular { .. } => {
            let dir = root.join("app");
            let mut cmd = Command::new("npm");
            cmd.args(["install", &format!("erno-angular@{TARGET_ERNO_ANGULAR}")])
                .current_dir(&dir);
            apply_ci(&mut cmd);
            run_prefixed(&mut cmd, "app")
        }
        StepKind::ErnoCrate { spec } => match spec {
            plan::CrateSpec::Path(_) => {
                ui::warn("path dependency — update the erno checkout, then rebuild");
                true
            }
            plan::CrateSpec::Git | plan::CrateSpec::Version(_) => {
                let dir = root.join("api");
                let mut cmd = Command::new("cargo");
                cmd.args(["update", "-p", "erno"]).current_dir(&dir);
                apply_ci(&mut cmd);
                run_prefixed(&mut cmd, "api")
            }
        },
    }
}

fn ng_update(dir: &Path, major: u32, force: bool) -> bool {
    let ng = local_ng(dir).unwrap_or_else(|| PathBuf::from("npx"));
    let mut cmd = Command::new(&ng);
    if ng.file_name().and_then(|s| s.to_str()) == Some("npx") {
        cmd.args(["--yes", "ng"]);
    }
    cmd.args([
        "update",
        &format!("@angular/core@{major}"),
        &format!("@angular/cli@{major}"),
    ]);
    if force {
        cmd.arg("--allow-dirty");
    }
    cmd.current_dir(dir);
    apply_ci(&mut cmd);
    run_prefixed(&mut cmd, "ng")
}

fn ionic_migrate(dir: &Path, force: bool) -> bool {
    let mut cmd = Command::new("npx");
    cmd.args(["--yes", "@ionic/migrate"]);
    if force {
        cmd.arg("--force");
    }
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
