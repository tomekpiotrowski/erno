//! The package manifest (`erno.toml`) and the shared runner behind
//! `erno build`, `erno lint`, and `erno test`.
//!
//! A project declares its packages once, in declaration order, and each package
//! declares the steps for each phase. Declaration order *is* execution order —
//! that is how build dependency order is expressed, and why there is no
//! dependency graph and no parallelism here.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::Args;
use serde::Deserialize;

use crate::ui;

/// Package selection flags, shared by `build`, `lint`, and `test`.
#[derive(Args, Debug, Default)]
pub struct SelectionArgs {
    /// Package from erno.toml (repeatable)
    #[arg(long)]
    pub package: Vec<String>,
    /// Include packages and steps marked `default = false`
    #[arg(long)]
    pub all: bool,
    /// Shorthand for `--package api`
    #[arg(long)]
    pub api: bool,
    /// Shorthand for `--package app`
    #[arg(long)]
    pub app: bool,
    /// Shorthand for `--package e2e`
    #[arg(long)]
    pub e2e: bool,
    /// Skip the e2e package even when it would otherwise run
    #[arg(long)]
    pub no_e2e: bool,
    /// Stop after the first failing package
    #[arg(long)]
    pub fail_fast: bool,
    /// Arguments forwarded to the single selected package's steps
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}

impl SelectionArgs {
    /// Every package name the flags name explicitly, `--api`-style sugar included.
    fn named(&self) -> Vec<String> {
        let mut names = self.package.clone();
        for (flag, name) in [(self.api, "api"), (self.app, "app"), (self.e2e, "e2e")] {
            if flag && !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
        names
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Build,
    Lint,
    Test,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Build => "build",
            Phase::Lint => "lint",
            Phase::Test => "test",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Phase::Build => ui::icon::BUILD,
            Phase::Lint => ui::icon::LINT,
            Phase::Test => ui::icon::TEST,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(default)]
    package: Vec<Package>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub dir: String,
    /// `false` means opt in with `--package <name>` or `--all`.
    #[serde(default = "default_true")]
    pub default: bool,
    /// Ensure the test database exists before this package's test phase.
    #[serde(default)]
    pub database: bool,
    /// Only `"e2e"` is recognised; the CLI orchestrates that one itself.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub build: Vec<Step>,
    #[serde(default)]
    pub lint: Vec<Step>,
    #[serde(default)]
    pub test: Vec<Step>,
    /// Long-running process for `erno dev`. At most one per package.
    #[serde(default)]
    pub dev: Vec<DevService>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevService {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub url: String,
    /// A binary that must be on `PATH` before this service is started.
    ///
    /// Without it a missing or misnamed binary is a restart loop with the same
    /// spawn error scrolling past, rather than one line naming what to install.
    #[serde(default)]
    pub requires: Option<String>,
    /// Ports to check are free, beyond the one in `url`.
    ///
    /// For a service that listens on more than one: Tempo answers queries on
    /// its `url` but receives OTLP on another port, and a Tempo that came up
    /// without that port bound looks healthy and accepts nothing.
    #[serde(default)]
    pub ports: Vec<u16>,
    /// `false` means opt in with `--all` (naming the package is not enough).
    #[serde(default = "default_true")]
    pub default: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Lint only: the argument vector substituted under `--fix`. A step with no
    /// `fix` runs its check form unchanged.
    #[serde(default)]
    pub fix: Vec<String>,
    /// `false` means opt in with `--package <name>` or `--all`.
    #[serde(default = "default_true")]
    pub default: bool,
}

fn default_true() -> bool {
    true
}

impl Package {
    pub fn steps(&self, phase: Phase) -> &[Step] {
        match phase {
            Phase::Build => &self.build,
            Phase::Lint => &self.lint,
            Phase::Test => &self.test,
        }
    }

    pub fn is_e2e(&self) -> bool {
        self.kind.as_deref() == Some("e2e")
    }
}

impl Step {
    /// The argument vector to run, honouring `--fix` when the step defines one.
    fn resolved_args(&self, fix: bool) -> &[String] {
        if fix && !self.fix.is_empty() {
            &self.fix
        } else {
            &self.args
        }
    }
}

/// Read `erno.toml` from the project root, or fall back to the conventional
/// layout so a freshly scaffolded project works with no manifest at all.
pub fn load_packages(root: &Path) -> Result<Vec<Package>, String> {
    let path = root.join("erno.toml");
    if !path.is_file() {
        return Ok(conventional(root));
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let manifest: Manifest =
        toml::from_str(&raw).map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    validate(&manifest.package)?;
    Ok(manifest.package)
}

fn validate(packages: &[Package]) -> Result<(), String> {
    let mut seen: Vec<&str> = Vec::new();
    for p in packages {
        if p.name.is_empty() || p.dir.is_empty() {
            return Err("each [[package]] needs a non-empty name and dir".into());
        }
        if seen.contains(&p.name.as_str()) {
            return Err(format!("duplicate package '{}'", p.name));
        }
        seen.push(&p.name);
        if let Some(kind) = &p.kind {
            if kind != "e2e" {
                return Err(format!(
                    "package '{}' has unknown kind '{kind}' (only \"e2e\" is recognised)",
                    p.name
                ));
            }
        }
        for step in p.build.iter().chain(&p.test) {
            if !step.fix.is_empty() {
                return Err(format!(
                    "package '{}' sets `fix` on a non-lint step; `fix` is lint-only",
                    p.name
                ));
            }
        }
        if p.build
            .iter()
            .chain(&p.lint)
            .chain(&p.test)
            .any(|s| s.command.is_empty())
        {
            return Err(format!(
                "package '{}' has a step with an empty command",
                p.name
            ));
        }
        if p.dev.len() > 1 {
            return Err(format!(
                "package '{}' has more than one [[package.dev]]; one long-running process per package",
                p.name
            ));
        }
        for d in &p.dev {
            if d.command.is_empty() {
                return Err(format!(
                    "package '{}' has a [[package.dev]] with an empty command",
                    p.name
                ));
            }
            if d.url.is_empty() {
                return Err(format!(
                    "package '{}' has a [[package.dev]] with an empty url",
                    p.name
                ));
            }
        }
    }
    Ok(())
}

/// The layout `erno new` scaffolds, for projects without an `erno.toml`.
fn conventional(root: &Path) -> Vec<Package> {
    let mut packages = Vec::new();

    if root.join("api").join("Cargo.toml").is_file() {
        packages.push(rust_package("api", "api"));
    }

    for (name, dir) in [("app", "app"), ("www", "www"), ("admin", "admin")] {
        if let Some(package) = npm_package(root, name, dir) {
            packages.push(package);
        }
    }

    if let Some(dir) = playwright_dir(root) {
        packages.push(Package {
            name: "e2e".into(),
            dir,
            default: true,
            database: true,
            kind: Some("e2e".into()),
            build: Vec::new(),
            lint: Vec::new(),
            test: Vec::new(),
            dev: Vec::new(),
        });
    }

    packages
}

/// A cargo crate laid out the way `erno new` scaffolds them.
fn rust_package(name: &str, dir: &str) -> Package {
    Package {
        name: name.into(),
        dir: dir.into(),
        default: true,
        database: true,
        kind: None,
        build: vec![step("cargo", &["build"])],
        lint: vec![
            lint_step("cargo", &["fmt", "--check"], &["fmt"]),
            lint_step(
                "cargo",
                &["clippy", "--all-targets", "--", "-D", "warnings"],
                &["clippy", "--all-targets", "--fix", "--allow-dirty"],
            ),
        ],
        test: vec![step("cargo", &["test"])],
        dev: Vec::new(),
    }
}

/// An npm project, with each phase gated on the script actually existing — a
/// project without a `lint` script must not grow a step that always fails.
///
/// `None` when there is no package.json, so a layout missing `www/` or
/// `admin/` simply has no such package.
fn npm_package(root: &Path, name: &str, dir: &str) -> Option<Package> {
    let path = root.join(dir);
    if !path.join("package.json").is_file() {
        return None;
    }
    let mut build = Vec::new();
    if has_npm_script(&path, "build") {
        build.push(step("npm", &["run", "build"]));
    }
    let mut lint = Vec::new();
    if has_npm_script(&path, "lint") {
        lint.push(step("npm", &["run", "lint"]));
    }
    let mut test = Vec::new();
    if has_npm_script(&path, "test:ci") {
        test.push(step("npm", &["run", "test:ci"]));
    } else if has_npm_script(&path, "test") {
        test.push(step("npm", &["test", "--", "--watch=false"]));
    }
    Some(Package {
        name: name.into(),
        dir: dir.into(),
        default: true,
        database: false,
        kind: None,
        build,
        lint,
        test,
        dev: Vec::new(),
    })
}

fn step(command: &str, args: &[&str]) -> Step {
    Step {
        command: command.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        fix: Vec::new(),
        default: true,
    }
}

fn lint_step(command: &str, args: &[&str], fix: &[&str]) -> Step {
    Step {
        fix: fix.iter().map(|a| a.to_string()).collect(),
        ..step(command, args)
    }
}

fn has_npm_script(dir: &Path, name: &str) -> bool {
    std::fs::read_to_string(dir.join("package.json"))
        .map(|raw| raw.contains(&format!("\"{name}\"")))
        .unwrap_or(false)
}

/// The directory holding the Playwright config, relative to the project root.
pub fn playwright_dir(root: &Path) -> Option<String> {
    if root.join("e2e").join("playwright.config.ts").is_file() {
        return Some("e2e".into());
    }
    if root.join("playwright.config.ts").is_file() {
        return Some(".".into());
    }
    None
}

pub fn select<'a>(all: &'a [Package], args: &SelectionArgs) -> Result<Vec<&'a Package>, String> {
    let known: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
    let named = args.named();
    for asked in &named {
        if !known.contains(&asked.as_str()) {
            return Err(format!(
                "unknown package '{asked}'. Known: {}",
                known.join(", ")
            ));
        }
    }

    let selected: Vec<&Package> = all
        .iter()
        .filter(|p| {
            if args.no_e2e && p.is_e2e() {
                return false;
            }
            if named.is_empty() {
                p.default || args.all
            } else {
                named.contains(&p.name)
            }
        })
        .collect();

    if selected.is_empty() {
        return Err("no packages selected".into());
    }
    if !args.rest.is_empty() && selected.len() != 1 {
        return Err(
            "pass-through arguments require exactly one package (use --package, --api, --app, or --e2e)"
                .into(),
        );
    }
    Ok(selected)
}

/// The steps of one phase that a run should actually execute.
///
/// Only `--all` pulls in `default = false` steps. Naming a package selects the
/// package, not its slow extras: `--package puzzles` should run the test suite,
/// not silently start a multi-minute release build alongside it.
fn steps_to_run(package: &Package, phase: Phase, all: bool) -> Vec<&Step> {
    package
        .steps(phase)
        .iter()
        .filter(|s| s.default || all)
        .collect()
}

/// Run one phase across the selected packages, in declaration order.
///
/// `special` gets first refusal on each package; returning `Some(ok)` means it
/// handled the package itself (that is how `erno test` runs the e2e
/// orchestration), and `None` falls through to the declared steps.
pub fn run_phase(
    root: &Path,
    selected: &[&Package],
    phase: Phase,
    fix: bool,
    args: &SelectionArgs,
    special: &mut dyn FnMut(&Package) -> Option<bool>,
) -> bool {
    let started = Instant::now();
    let mut results: Vec<(String, bool, Duration)> = Vec::new();

    for package in selected {
        let steps = steps_to_run(package, phase, args.all);

        let package_started = Instant::now();
        let ok = match special(package) {
            Some(handled) => handled,
            None => {
                if steps.is_empty() {
                    continue;
                }
                ui::section(phase.icon(), &package.name);
                run_steps(root, package, &steps, fix, &args.rest)
            }
        };

        results.push((package.name.clone(), ok, package_started.elapsed()));
        if !ok && args.fail_fast {
            break;
        }
    }

    if results.is_empty() {
        ui::info(format!("nothing to {}", phase.label()));
        return true;
    }

    // The summary is the command's result, not narration, so `--quiet` keeps it
    // — which is why it goes through `emit` rather than `ui::ok`/`ui::fail`.
    let rows: Vec<ui::Row> = results
        .iter()
        .map(|(name, ok, took)| ui::Row {
            level: if *ok { ui::Level::Ok } else { ui::Level::Fail },
            label: name.clone(),
            detail: Some(ui::fmt_duration(*took)),
            hint: None,
        })
        .collect();
    ui::emit(ui::Stream::Err, "");
    ui::emit_block(
        ui::Stream::Err,
        &ui::render_rows(ui::Face::current(), &rows),
    );

    let total = ui::fmt_duration(started.elapsed());
    let failures = results.iter().filter(|(_, ok, _)| !ok).count();
    if failures > 0 {
        ui::emit(ui::Stream::Err, "");
        ui::fatal(&format!(
            "{failures} of {} packages failed in {total}",
            results.len()
        ));
        return false;
    }
    ui::finished(
        ui::icon::DONE,
        format!("{} finished in {total}", phase.label()),
    );
    true
}

fn run_steps(root: &Path, package: &Package, steps: &[&Step], fix: bool, rest: &[String]) -> bool {
    let dir = root.join(&package.dir);
    if dir.join("package.json").is_file() && !ensure_npm_modules(&dir, &package.name) {
        return false;
    }
    // Sized up front so every step's time sits in the same column, however
    // long the command lines are.
    let labels: Vec<String> = steps
        .iter()
        .map(|step| step_label(step, fix, rest))
        .collect();
    let width = ui::column_width(labels.iter().map(String::as_str));

    for (step, label) in steps.iter().zip(&labels) {
        let mut cmd = Command::new(&step.command);
        cmd.args(step.resolved_args(fix))
            .args(rest)
            .current_dir(&dir);

        let started = Instant::now();
        let ok = run_prefixed(&mut cmd, &package.name);

        // The step's own output has already scrolled past by now, so this row
        // is what says which step that was and where the time went. A failing
        // one is a result, not narration, so `--quiet` keeps it.
        let text = format!("{label:<width$}  {}", ui::fmt_duration(started.elapsed()));
        if ok {
            ui::ok(text);
        } else {
            ui::emit(
                ui::Stream::Err,
                &ui::render_row(ui::Face::current(), ui::Level::Fail, &text),
            );
            return false;
        }
    }
    true
}

/// A step as the user would have typed it, for the timing row.
fn step_label(step: &Step, fix: bool, rest: &[String]) -> String {
    std::iter::once(step.command.clone())
        .chain(step.resolved_args(fix).iter().cloned())
        .chain(rest.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `node_modules` is gitignored, so install on first use rather than failing.
pub fn ensure_npm_modules(dir: &Path, label: &str) -> bool {
    if dir.join("node_modules").is_dir() {
        return true;
    }
    if !dir.join("package.json").is_file() {
        ui::prefixed(
            ui::Stream::Err,
            label,
            &format!("no package.json in {}", dir.display()),
        );
        return false;
    }
    ui::prefixed(
        ui::Stream::Err,
        label,
        &format!("npm install in {}", dir.display()),
    );
    let mut cmd = Command::new("npm");
    cmd.arg("install").current_dir(dir);
    let ok = run_prefixed(&mut cmd, label);
    if !ok {
        ui::prefixed(
            ui::Stream::Err,
            label,
            &format!("npm install failed in {}", dir.display()),
        );
    }
    ok
}

pub fn run_prefixed(cmd: &mut Command, label: &str) -> bool {
    ui::apply_child_env(cmd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            ui::prefixed(ui::Stream::Err, label, &format!("failed to start: {e}"));
            return false;
        }
    };
    if let Some(out) = child.stdout.take() {
        prefix_pipe(out, label, ui::Stream::Out);
    }
    if let Some(err) = child.stderr.take() {
        prefix_pipe(err, label, ui::Stream::Err);
    }
    match child.wait() {
        Ok(status) => status.success(),
        Err(e) => {
            ui::prefixed(ui::Stream::Err, label, &format!("wait failed: {e}"));
            false
        }
    }
}

/// Forward a child's pipe, one prefixed line at a time, onto the stream it came
/// from.
pub fn prefix_pipe<R: std::io::Read + Send + 'static>(pipe: R, label: &str, stream: ui::Stream) {
    use std::io::BufRead;
    let label = label.to_string();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(pipe);
        for line in reader.lines().map_while(Result::ok) {
            ui::prefixed(stream, &label, &line);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "erno-packages-{}-{}-{suffix}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn write_manifest(root: &Path, body: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join("erno.toml"), body).unwrap();
    }

    const MANIFEST: &str = r#"
[[package]]
name = "puzzles"
dir  = "puzzles"

  [[package.build]]
  command = "./build.sh"

  [[package.lint]]
  command = "cargo"
  args    = ["fmt", "--check"]
  fix     = ["fmt"]

  [[package.test]]
  command = "cargo"
  args    = ["test"]

  [[package.test]]
  command = "cargo"
  args    = ["test", "--release", "--", "--ignored"]
  default = false

[[package]]
name = "app"
dir  = "app"

  [[package.build]]
  command = "npm"
  args    = ["run", "build"]

[[package]]
name = "vision"
dir  = "vision"
default = false

  [[package.test]]
  command = "cargo"
  args    = ["test"]

  [[package.dev]]
  command = "./serve.sh"
  url     = "http://localhost:8765/tools/solve_studio/"
"#;

    #[test]
    fn parses_all_phases_and_keeps_declaration_order() {
        let root = temp("parse");
        write_manifest(&root, MANIFEST);
        let packages = load_packages(&root).unwrap();
        assert_eq!(
            packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["puzzles", "app", "vision"]
        );
        let puzzles = &packages[0];
        assert_eq!(puzzles.build.len(), 1);
        assert_eq!(puzzles.build[0].command, "./build.sh");
        assert_eq!(puzzles.lint[0].fix, ["fmt"]);
        assert_eq!(puzzles.test.len(), 2);
        assert!(puzzles.test[0].default);
        assert!(!puzzles.test[1].default);
        assert!(packages[1].test.is_empty());
        let vision = &packages[2];
        assert_eq!(vision.dev.len(), 1);
        assert_eq!(vision.dev[0].command, "./serve.sh");
        assert_eq!(
            vision.dev[0].url,
            "http://localhost:8765/tools/solve_studio/"
        );
        assert!(vision.dev[0].args.is_empty());
        assert!(vision.dev[0].default);
        assert!(packages[0].dev.is_empty());
        assert!(packages[1].dev.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn falls_back_to_conventions_without_a_manifest() {
        let root = temp("conv");
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join("api/Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(
            root.join("app/package.json"),
            "{\"scripts\":{\"test:ci\":\"x\"}}",
        )
        .unwrap();
        fs::create_dir_all(root.join("e2e")).unwrap();
        fs::write(root.join("e2e/playwright.config.ts"), "export default {}\n").unwrap();

        let packages = load_packages(&root).unwrap();
        assert_eq!(
            packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["api", "app", "e2e"]
        );
        assert_eq!(packages[0].build[0].args, ["build"]);
        assert_eq!(packages[1].test[0].args, ["run", "test:ci"]);
        assert!(packages[2].is_e2e());
        assert!(packages.iter().all(|p| p.dev.is_empty()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn default_selection_skips_opt_in_packages() {
        let root = temp("select");
        write_manifest(&root, MANIFEST);
        let packages = load_packages(&root).unwrap();

        let selected = select(&packages, &SelectionArgs::default()).unwrap();
        assert_eq!(
            selected.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["puzzles", "app"]
        );

        let all = select(
            &packages,
            &SelectionArgs {
                all: true,
                ..SelectionArgs::default()
            },
        )
        .unwrap();
        assert_eq!(all.len(), 3);

        let named = select(
            &packages,
            &SelectionArgs {
                package: vec!["vision".into()],
                ..SelectionArgs::default()
            },
        )
        .unwrap();
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].name, "vision");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_package_lists_known() {
        let root = temp("unknown");
        write_manifest(&root, MANIFEST);
        let packages = load_packages(&root).unwrap();
        let err = select(
            &packages,
            &SelectionArgs {
                package: vec!["nope".into()],
                ..SelectionArgs::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("nope"));
        assert!(err.contains("puzzles"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn only_all_pulls_in_opt_in_steps() {
        let root = temp("steps");
        write_manifest(&root, MANIFEST);
        let packages = load_packages(&root).unwrap();
        let puzzles = &packages[0];

        // Naming the package must not drag in its slow release guard.
        let default_run = steps_to_run(puzzles, Phase::Test, false);
        assert_eq!(default_run.len(), 1);
        assert_eq!(default_run[0].args, ["test"]);

        let full_run = steps_to_run(puzzles, Phase::Test, true);
        assert_eq!(full_run.len(), 2);
        assert_eq!(full_run[1].args, ["test", "--release", "--", "--ignored"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fix_substitutes_args_and_falls_back() {
        let with_fix = lint_step("cargo", &["fmt", "--check"], &["fmt"]);
        assert_eq!(with_fix.resolved_args(false), ["fmt", "--check"]);
        assert_eq!(with_fix.resolved_args(true), ["fmt"]);

        let without_fix = step("npm", &["run", "lint"]);
        assert_eq!(without_fix.resolved_args(true), ["run", "lint"]);
    }

    #[test]
    fn pass_through_requires_one_package() {
        let root = temp("passthrough");
        write_manifest(&root, MANIFEST);
        let packages = load_packages(&root).unwrap();
        let err = select(
            &packages,
            &SelectionArgs {
                rest: vec!["health".into()],
                ..SelectionArgs::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("pass-through"));

        let ok = select(
            &packages,
            &SelectionArgs {
                package: vec!["puzzles".into()],
                rest: vec!["health".into()],
                ..SelectionArgs::default()
            },
        )
        .unwrap();
        assert_eq!(ok.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sugar_flags_resolve_to_package_names() {
        let root = temp("sugar");
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join("api/Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app/package.json"), "{}").unwrap();
        let packages = load_packages(&root).unwrap();
        let selected = select(
            &packages,
            &SelectionArgs {
                api: true,
                ..SelectionArgs::default()
            },
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "api");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn no_e2e_drops_the_e2e_package() {
        let root = temp("noe2e");
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join("api/Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::create_dir_all(root.join("e2e")).unwrap();
        fs::write(root.join("e2e/playwright.config.ts"), "export default {}\n").unwrap();
        let packages = load_packages(&root).unwrap();
        let selected = select(
            &packages,
            &SelectionArgs {
                no_e2e: true,
                ..SelectionArgs::default()
            },
        )
        .unwrap();
        assert_eq!(
            selected.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["api"]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_unknown_kind_and_duplicate_names() {
        let root = temp("bad-kind");
        write_manifest(
            &root,
            "[[package]]\nname = \"x\"\ndir = \"x\"\nkind = \"nope\"\n",
        );
        assert!(load_packages(&root).unwrap_err().contains("unknown kind"));

        let dup = temp("dup");
        write_manifest(
            &dup,
            "[[package]]\nname = \"x\"\ndir = \"a\"\n\n[[package]]\nname = \"x\"\ndir = \"b\"\n",
        );
        assert!(load_packages(&dup).unwrap_err().contains("duplicate"));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&dup);
    }

    #[test]
    fn rejects_unknown_keys() {
        let root = temp("typo");
        write_manifest(
            &root,
            "[[package]]\nname = \"x\"\ndir = \"x\"\ndefualt = false\n",
        );
        let err = load_packages(&root).unwrap_err();
        assert!(err.contains("defualt"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_fix_on_package_dev() {
        let root = temp("dev-fix");
        write_manifest(
            &root,
            "[[package]]\nname = \"x\"\ndir = \"x\"\n\n  [[package.dev]]\n  command = \"./s\"\n  url = \"http://localhost:1\"\n  fix = [\"x\"]\n",
        );
        let err = load_packages(&root).unwrap_err();
        assert!(err.contains("fix"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_empty_dev_command_or_url() {
        let root = temp("dev-empty-cmd");
        write_manifest(
            &root,
            "[[package]]\nname = \"x\"\ndir = \"x\"\n\n  [[package.dev]]\n  command = \"\"\n  url = \"http://localhost:1\"\n",
        );
        assert!(load_packages(&root).unwrap_err().contains("empty command"));
        let _ = fs::remove_dir_all(&root);

        let url = temp("dev-empty-url");
        write_manifest(
            &url,
            "[[package]]\nname = \"x\"\ndir = \"x\"\n\n  [[package.dev]]\n  command = \"./s\"\n  url = \"\"\n",
        );
        assert!(load_packages(&url).unwrap_err().contains("empty url"));
        let _ = fs::remove_dir_all(&url);
    }

    #[test]
    fn rejects_two_package_dev_entries() {
        let root = temp("dev-two");
        write_manifest(
            &root,
            "[[package]]\nname = \"x\"\ndir = \"x\"\n\n  [[package.dev]]\n  command = \"./a\"\n  url = \"http://localhost:1\"\n\n  [[package.dev]]\n  command = \"./b\"\n  url = \"http://localhost:2\"\n",
        );
        let err = load_packages(&root).unwrap_err();
        assert!(err.contains("one long-running process"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }
}
