//! Inventory of Erno-managed packages and the steps that would update them.
//!
//! Targets come from this CLI generation, not a hardcoded "go to 22" script:
//! raising [`TARGET_ANGULAR_MAJOR`] is how the next Erno release moves.

use serde_json::Value as Json;

/// Angular major this CLI scaffolds and upgrades toward.
pub const TARGET_ANGULAR_MAJOR: u32 = 22;
/// Ionic major this CLI scaffolds and upgrades toward.
pub const TARGET_IONIC_MAJOR: u32 = 9;
/// `erno-angular` version this CLI was built against.
pub const TARGET_ERNO_ANGULAR: &str = "0.0.1";

/// Angular 22's Node floor: `^22.22.3 || ^24.15.0 || ^26`.
pub fn node_is_supported(major: u32, minor: u32, patch: u32) -> bool {
    match major {
        22 => minor > 22 || (minor == 22 && patch >= 3),
        24 => minor >= 15,
        26.. => true,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum GitStatus {
    #[default]
    Clean,
    Dirty,
    /// `git` is not on PATH.
    GitMissing,
    /// Directory is not inside a git work tree.
    NotARepo,
}

impl GitStatus {
    pub fn blocking_message(&self) -> Option<&'static str> {
        match self {
            GitStatus::Clean => None,
            GitStatus::Dirty => {
                Some("working tree is dirty — commit or stash, or pass --force")
            }
            GitStatus::GitMissing => {
                Some("git not found — erno upgrade requires git so you can undo; pass --force to skip")
            }
            GitStatus::NotARepo => {
                Some("not a git repository — erno upgrade requires git so you can undo; pass --force to skip")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrateSpec {
    Git,
    Version(String),
    Path(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    Angular { dir: String, from_major: u32 },
    Ionic { dir: String, from_major: u32 },
    ErnoAngular { from: String },
    ErnoCrate { spec: CrateSpec },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeStep {
    pub label: String,
    pub current: String,
    pub target: String,
    pub how: String,
    pub kind: StepKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradePlan {
    /// Conditions that refuse execution (Node too old, dirty git).
    pub blocking: Vec<String>,
    pub steps: Vec<UpgradeStep>,
}

impl UpgradePlan {
    pub fn is_current(&self) -> bool {
        self.blocking.is_empty() && self.steps.is_empty()
    }
}

/// Inputs the planner reads. Tests pass fixtures; the command fills this from disk.
#[derive(Debug, Default)]
pub struct ProjectSnapshot {
    pub node: Option<String>,
    pub git: GitStatus,
    pub force: bool,
    pub app_package: Option<String>,
    pub admin_package: Option<String>,
    pub api_cargo: Option<String>,
}

pub fn plan_upgrade(snap: &ProjectSnapshot) -> UpgradePlan {
    let mut blocking = Vec::new();
    if !snap.force {
        if let Some(msg) = snap.git.blocking_message() {
            blocking.push(msg.to_string());
        }
    }
    match parse_node(snap.node.as_deref()) {
        None => blocking.push("Node.js not found — install Node 22.22.3 or later".into()),
        Some((maj, min, pat)) if !node_is_supported(maj, min, pat) => blocking.push(format!(
            "Node.js {maj}.{min}.{pat} is too old — Angular {TARGET_ANGULAR_MAJOR} needs ^22.22.3 || ^24.15.0 || ^26"
        )),
        Some(_) => {}
    }

    let mut steps = Vec::new();
    if let Some(pkg) = snap.app_package.as_deref() {
        steps.extend(scan_app(pkg));
    }
    if let Some(pkg) = snap.admin_package.as_deref() {
        steps.extend(scan_admin(pkg));
    }
    if let Some(cargo) = snap.api_cargo.as_deref() {
        if let Some(step) = scan_api(cargo) {
            steps.push(step);
        }
    }

    UpgradePlan { blocking, steps }
}

fn scan_app(pkg_json: &str) -> Vec<UpgradeStep> {
    let Ok(pkg) = serde_json::from_str::<Json>(pkg_json) else {
        return Vec::new();
    };
    let mut steps = Vec::new();
    let angular_behind = match dep_major(&pkg, "@angular/core") {
        Some((current, major)) if major < TARGET_ANGULAR_MAJOR => {
            steps.push(angular_step("app", "app", &current, major));
            true
        }
        _ => false,
    };
    let ionic_behind = match dep_major(&pkg, "@ionic/angular") {
        Some((current, major)) if major < TARGET_IONIC_MAJOR => {
            steps.push(UpgradeStep {
                label: "app Ionic".into(),
                current,
                target: format!("{TARGET_IONIC_MAJOR}.x"),
                how: "npx --yes @ionic/migrate".into(),
                kind: StepKind::Ionic {
                    dir: "app".into(),
                    from_major: major,
                },
            });
            true
        }
        _ => false,
    };
    if let Some(from) = dep_raw(&pkg, "erno-angular") {
        let local = from.starts_with("file:") || from.starts_with('/') || from.starts_with('.');
        if !local && (angular_behind || ionic_behind || version_name(&from) != TARGET_ERNO_ANGULAR)
        {
            steps.push(UpgradeStep {
                label: "app erno-angular".into(),
                current: from.clone(),
                target: TARGET_ERNO_ANGULAR.into(),
                how: format!("npm install erno-angular@{TARGET_ERNO_ANGULAR}"),
                kind: StepKind::ErnoAngular { from },
            });
        }
    }
    steps
}

fn scan_admin(pkg_json: &str) -> Vec<UpgradeStep> {
    let Ok(pkg) = serde_json::from_str::<Json>(pkg_json) else {
        return Vec::new();
    };
    match dep_major(&pkg, "@angular/core") {
        Some((current, major)) if major < TARGET_ANGULAR_MAJOR => {
            vec![angular_step("admin", "admin", &current, major)]
        }
        _ => Vec::new(),
    }
}

fn scan_api(cargo_toml: &str) -> Option<UpgradeStep> {
    let spec = parse_erno_dep(cargo_toml)?;
    match spec {
        CrateSpec::Path(p) => Some(UpgradeStep {
            label: "api erno".into(),
            current: format!("path = {p}"),
            target: "this checkout".into(),
            how: "path dependency — upgrade the erno checkout yourself".into(),
            kind: StepKind::ErnoCrate {
                spec: CrateSpec::Path(p),
            },
        }),
        CrateSpec::Git => Some(UpgradeStep {
            label: "api erno".into(),
            current: "git".into(),
            target: "latest".into(),
            how: "cargo update -p erno".into(),
            kind: StepKind::ErnoCrate {
                spec: CrateSpec::Git,
            },
        }),
        CrateSpec::Version(v) => Some(UpgradeStep {
            label: "api erno".into(),
            current: v.clone(),
            target: "latest".into(),
            how: "cargo update -p erno".into(),
            kind: StepKind::ErnoCrate {
                spec: CrateSpec::Version(v),
            },
        }),
    }
}

fn angular_step(label_prefix: &str, dir: &str, current: &str, from_major: u32) -> UpgradeStep {
    let majors: Vec<String> = ((from_major + 1)..=TARGET_ANGULAR_MAJOR)
        .map(|m| m.to_string())
        .collect();
    let n = majors.len();
    let how = if n == 1 {
        format!("ng update @angular/core@{0} @angular/cli@{0}", majors[0])
    } else {
        format!(
            "ng update, {n} majors ({})",
            majors
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let from = from_major + i as u32;
                    format!("{from}→{m}")
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    UpgradeStep {
        label: format!("{label_prefix} Angular"),
        current: current.into(),
        target: format!("{TARGET_ANGULAR_MAJOR}.x"),
        how,
        kind: StepKind::Angular {
            dir: dir.into(),
            from_major,
        },
    }
}

pub fn angular_majors(from_major: u32) -> Vec<u32> {
    ((from_major + 1)..=TARGET_ANGULAR_MAJOR).collect()
}

fn dep_raw(pkg: &Json, name: &str) -> Option<String> {
    for section in ["dependencies", "devDependencies"] {
        if let Some(v) = pkg.get(section)?.get(name)?.as_str() {
            return Some(v.to_string());
        }
    }
    None
}

fn dep_major(pkg: &Json, name: &str) -> Option<(String, u32)> {
    let raw = dep_raw(pkg, name)?;
    let major = parse_major(&raw)?;
    Some((raw, major))
}

/// First numeric component of `^20.2.0`, `~8.8.7`, `20.3.21`, `file:…`.
pub fn parse_major(spec: &str) -> Option<u32> {
    let s = spec.trim();
    if s.starts_with("file:") || s.starts_with("git") || s == "*" {
        return None;
    }
    let digits = s.trim_start_matches(|c: char| !c.is_ascii_digit());
    let major: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    major.parse().ok()
}

fn version_name(spec: &str) -> String {
    spec.trim()
        .trim_start_matches(['^', '~', '=', 'v'])
        .to_string()
}

pub fn parse_node(raw: Option<&str>) -> Option<(u32, u32, u32)> {
    let s = raw?.trim().trim_start_matches('v');
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()
        .and_then(|p| {
            p.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .unwrap_or(0);
    Some((major, minor, patch))
}

/// Very small Cargo.toml scrape: the `erno = …` dependency line / table.
fn parse_erno_dep(cargo_toml: &str) -> Option<CrateSpec> {
    // Inline: erno = { git = "…" } / { path = "…" } / "1.2.3"
    for line in cargo_toml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("erno") {
            let rest = rest.trim_start().trim_start_matches('=').trim();
            if rest.starts_with('{') {
                if rest.contains("path") {
                    let path = rest
                        .split("path")
                        .nth(1)?
                        .trim_start_matches([' ', '=', '"', '\''].as_slice());
                    let path: String = path
                        .chars()
                        .take_while(|c| *c != '"' && *c != '\'')
                        .collect();
                    return Some(CrateSpec::Path(path));
                }
                if rest.contains("git") {
                    return Some(CrateSpec::Git);
                }
                if rest.contains("version") {
                    let v = rest
                        .split("version")
                        .nth(1)?
                        .trim_start_matches([' ', '=', '"', '\''].as_slice());
                    let v: String = v.chars().take_while(|c| *c != '"' && *c != '\'').collect();
                    return Some(CrateSpec::Version(v));
                }
            } else if rest.starts_with('"') {
                let v: String = rest
                    .trim_start_matches('"')
                    .chars()
                    .take_while(|c| *c != '"')
                    .collect();
                return Some(CrateSpec::Version(v));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP_20: &str = r#"{
      "dependencies": {
        "@angular/core": "^20.3.21",
        "@ionic/angular": "^8.8.7",
        "erno-angular": "^0.0.1"
      }
    }"#;

    const APP_CURRENT: &str = r#"{
      "dependencies": {
        "@angular/core": "^22.1.3",
        "@ionic/angular": "^9.0.0",
        "erno-angular": "0.0.1"
      }
    }"#;

    const ADMIN_20: &str = r#"{
      "dependencies": { "@angular/core": "^20.3.27" }
    }"#;

    const API_GIT: &str = r#"
[dependencies]
erno = { git = "https://github.com/tomekpiotrowski/erno" }
"#;

    const API_PATH: &str = r#"
[dependencies]
erno = { path = "/home/me/erno/api" }
"#;

    fn snap() -> ProjectSnapshot {
        ProjectSnapshot {
            node: Some("v22.23.2".into()),
            git: GitStatus::Clean,
            force: false,
            ..ProjectSnapshot::default()
        }
    }

    #[test]
    fn angular_20_app_lists_two_majors_ionic_and_erno_angular() {
        let mut s = snap();
        s.app_package = Some(APP_20.into());
        s.admin_package = Some(ADMIN_20.into());
        s.api_cargo = Some(API_GIT.into());
        let plan = plan_upgrade(&s);
        assert!(plan.blocking.is_empty(), "{:?}", plan.blocking);
        let labels: Vec<_> = plan.steps.iter().map(|st| st.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "app Angular",
                "app Ionic",
                "app erno-angular",
                "admin Angular",
                "api erno"
            ]
        );
        match &plan.steps[0].kind {
            StepKind::Angular { from_major, dir } => {
                assert_eq!(*from_major, 20);
                assert_eq!(dir, "app");
                assert_eq!(angular_majors(20), vec![21, 22]);
            }
            other => panic!("{other:?}"),
        }
        assert!(plan.steps[0].how.contains("20→21"));
        assert!(plan.steps[0].how.contains("21→22"));
    }

    #[test]
    fn local_file_erno_angular_is_not_a_registry_bump() {
        let mut s = snap();
        s.app_package = Some(
            r#"{
              "dependencies": {
                "@angular/core": "^22.1.3",
                "@ionic/angular": "^9.0.0",
                "erno-angular": "file:/tmp/erno-angular"
              }
            }"#
            .into(),
        );
        let plan = plan_upgrade(&s);
        assert!(plan.steps.is_empty(), "{:?}", plan.steps);
    }

    #[test]
    fn already_current_is_empty() {
        let mut s = snap();
        s.app_package = Some(APP_CURRENT.into());
        s.admin_package = Some(r#"{"dependencies":{"@angular/core":"^22.1.3"}}"#.into());
        // git erno crate still shows as updatable — exclude it for this case
        let plan = plan_upgrade(&s);
        assert!(plan.blocking.is_empty());
        assert!(plan.steps.is_empty(), "{:?}", plan.steps);
        assert!(plan.is_current());
    }

    #[test]
    fn no_admin_omits_admin_row() {
        let mut s = snap();
        s.app_package = Some(APP_20.into());
        let plan = plan_upgrade(&s);
        assert!(plan.steps.iter().all(|st| !st.label.starts_with("admin")));
    }

    #[test]
    fn old_node_blocks() {
        let mut s = snap();
        s.node = Some("v22.17.1".into());
        s.app_package = Some(APP_20.into());
        let plan = plan_upgrade(&s);
        assert_eq!(plan.blocking.len(), 1);
        assert!(plan.blocking[0].contains("too old"));
        // inventory still listed so --dry-run is useful
        assert!(!plan.steps.is_empty());
    }

    #[test]
    fn dirty_git_blocks_unless_force() {
        let mut s = snap();
        s.git = GitStatus::Dirty;
        s.app_package = Some(APP_20.into());
        let plan = plan_upgrade(&s);
        assert!(plan.blocking.iter().any(|b| b.contains("dirty")));
        s.force = true;
        let plan = plan_upgrade(&s);
        assert!(plan.blocking.is_empty());
    }

    #[test]
    fn missing_git_is_not_called_dirty() {
        let mut s = snap();
        s.git = GitStatus::GitMissing;
        let plan = plan_upgrade(&s);
        assert!(plan.blocking.iter().any(|b| b.contains("git not found")));
        assert!(plan.blocking.iter().all(|b| !b.contains("dirty")));
        s.git = GitStatus::NotARepo;
        let plan = plan_upgrade(&s);
        assert!(plan
            .blocking
            .iter()
            .any(|b| b.contains("not a git repository")));
        assert!(plan.blocking.iter().all(|b| !b.contains("dirty")));
        s.force = true;
        let plan = plan_upgrade(&s);
        assert!(plan.blocking.is_empty());
    }

    #[test]
    fn path_erno_is_a_hint_not_cargo_update() {
        let mut s = snap();
        s.api_cargo = Some(API_PATH.into());
        let plan = plan_upgrade(&s);
        assert_eq!(plan.steps.len(), 1);
        assert!(plan.steps[0].how.contains("path dependency"));
        match &plan.steps[0].kind {
            StepKind::ErnoCrate {
                spec: CrateSpec::Path(p),
            } => assert!(p.contains("erno/api")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parse_major_strips_caret() {
        assert_eq!(parse_major("^20.3.21"), Some(20));
        assert_eq!(parse_major("~8.8.7"), Some(8));
        assert_eq!(parse_major("22.1.3"), Some(22));
        assert_eq!(parse_major("file:../erno-angular"), None);
    }

    #[test]
    fn node_floor() {
        assert!(!node_is_supported(22, 17, 1));
        assert!(node_is_supported(22, 22, 3));
        assert!(node_is_supported(22, 23, 2));
        assert!(!node_is_supported(24, 2, 0));
        assert!(node_is_supported(24, 15, 0));
        assert!(node_is_supported(26, 0, 0));
        assert!(!node_is_supported(20, 19, 2));
    }
}
