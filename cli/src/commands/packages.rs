//! The package manifest (`erno.toml`) and the shared runner behind
//! `erno build`, `erno lint`, and `erno test`.
//!
//! A project declares its packages once, in declaration order, and each package
//! declares the steps for each phase. Declaration order *is* execution order —
//! that is how build dependency order is expressed, and why there is no
//! dependency graph and no parallelism here.

use std::path::Path;
use std::process::{Command, Stdio};

use clap::Args;
use serde::Deserialize;

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
    }
    Ok(())
}

/// The layout `erno new` scaffolds, for projects without an `erno.toml`.
fn conventional(root: &Path) -> Vec<Package> {
    let mut packages = Vec::new();

    if root.join("api").join("Cargo.toml").is_file() {
        packages.push(Package {
            name: "api".into(),
            dir: "api".into(),
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
        });
    }

    let app = root.join("app");
    if app.join("package.json").is_file() {
        let test = if has_npm_script(&app, "test:ci") {
            step("npm", &["run", "test:ci"])
        } else {
            step(
                "npm",
                &["test", "--", "--watch=false", "--browsers=ChromeHeadless"],
            )
        };
        let mut build = Vec::new();
        if has_npm_script(&app, "build") {
            build.push(step("npm", &["run", "build"]));
        }
        let mut lint = Vec::new();
        if has_npm_script(&app, "lint") {
            lint.push(step("npm", &["run", "lint"]));
        }
        packages.push(Package {
            name: "app".into(),
            dir: "app".into(),
            default: true,
            database: false,
            kind: None,
            build,
            lint,
            test: vec![test],
        });
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
        });
    }

    packages
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
    let mut results: Vec<(String, bool)> = Vec::new();

    for package in selected {
        let steps = steps_to_run(package, phase, args.all);

        let ok = match special(package) {
            Some(handled) => handled,
            None => {
                if steps.is_empty() {
                    continue;
                }
                println!("\n── {} ──", package.name);
                run_steps(root, package, &steps, fix, &args.rest)
            }
        };

        results.push((package.name.clone(), ok));
        if !ok && args.fail_fast {
            break;
        }
    }

    if results.is_empty() {
        println!("nothing to {}", phase.label());
        return true;
    }

    println!();
    let mut failed = false;
    for (name, ok) in &results {
        if *ok {
            println!("  {name:<12} ok");
        } else {
            println!("  {name:<12} fail");
            failed = true;
        }
    }
    !failed
}

fn run_steps(root: &Path, package: &Package, steps: &[&Step], fix: bool, rest: &[String]) -> bool {
    let dir = root.join(&package.dir);
    if dir.join("package.json").is_file() && !ensure_npm_modules(&dir, &package.name) {
        return false;
    }
    for step in steps {
        let mut cmd = Command::new(&step.command);
        cmd.args(step.resolved_args(fix))
            .args(rest)
            .current_dir(&dir);
        if !run_prefixed(&mut cmd, &package.name) {
            return false;
        }
    }
    true
}

/// `node_modules` is gitignored, so install on first use rather than failing.
pub fn ensure_npm_modules(dir: &Path, label: &str) -> bool {
    if dir.join("node_modules").is_dir() {
        return true;
    }
    if !dir.join("package.json").is_file() {
        eprintln!("[{label}] no package.json in {}", dir.display());
        return false;
    }
    eprintln!("[{label}] npm install in {}", dir.display());
    let mut cmd = Command::new("npm");
    cmd.arg("install").current_dir(dir);
    let ok = run_prefixed(&mut cmd, label);
    if !ok {
        eprintln!("[{label}] npm install failed in {}", dir.display());
    }
    ok
}

/// Children see a pipe rather than a TTY, so ask them to keep colour anyway.
pub fn apply_child_color_env(cmd: &mut Command) {
    cmd.env("CARGO_TERM_COLOR", "always");
    cmd.env("FORCE_COLOR", "1");
    cmd.env("CLICOLOR_FORCE", "1");
    cmd.env("npm_config_color", "always");
    if std::env::var_os("TERM").is_none() {
        cmd.env("TERM", "xterm-256color");
    }
}

pub fn run_prefixed(cmd: &mut Command, label: &str) -> bool {
    apply_child_color_env(cmd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[{label}] failed to start: {e}");
            return false;
        }
    };
    if let Some(out) = child.stdout.take() {
        prefix_pipe(out, label);
    }
    if let Some(err) = child.stderr.take() {
        prefix_pipe(err, label);
    }
    match child.wait() {
        Ok(status) => status.success(),
        Err(e) => {
            eprintln!("[{label}] wait failed: {e}");
            false
        }
    }
}

pub fn prefix_pipe<R: std::io::Read + Send + 'static>(pipe: R, label: &str) {
    use std::io::BufRead;
    let label = label.to_string();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(pipe);
        for line in reader.lines().map_while(Result::ok) {
            println!("[{label}] {line}");
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
}
