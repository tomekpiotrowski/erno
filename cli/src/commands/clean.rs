use std::collections::BTreeSet;
use std::path::Path;

use clap::Args;

use crate::commands::dev::resolve_project_root;
use crate::commands::packages::Package;
use crate::ui;

const ARTIFACTS: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    ".angular",
    ".astro",
    "test-results",
    "playwright-report",
];

const CONVENTIONAL_DIRS: &[&str] = &["api", "app", "www", "admin", "e2e"];

/// Project-relative artifact directories that exist and are safe to delete.
fn collect_dirs(root: &Path, packages: &[Package]) -> Vec<String> {
    let mut bases: BTreeSet<String> = CONVENTIONAL_DIRS.iter().map(|s| (*s).to_string()).collect();
    for package in packages {
        if !package.dir.is_empty() {
            bases.insert(package.dir.clone());
        }
    }

    let mut found = BTreeSet::new();
    if root.join(".erno").is_dir() {
        found.insert(".erno".to_string());
    }
    for base in bases {
        for artifact in ARTIFACTS {
            let rel = format!("{base}/{artifact}");
            if root.join(&rel).is_dir() {
                found.insert(rel);
            }
        }
    }
    found.into_iter().collect()
}

#[derive(Args, Debug, Default)]
pub struct CleanArgs {
    /// Print the plan and exit
    #[arg(long)]
    pub dry_run: bool,
    /// Run without prompting
    #[arg(long)]
    pub yes: bool,
}

pub async fn handle_clean(_args: CleanArgs) -> ui::Cmd {
    let _root = resolve_project_root(None)?;
    ui::section(ui::icon::CLEAN, "Clean");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::commands::packages::Package;

    fn temp(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "erno-clean-{}-{}-{suffix}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pkg(name: &str, dir: &str) -> Package {
        Package {
            name: name.into(),
            dir: dir.into(),
            default: true,
            database: false,
            kind: None,
            build: Vec::new(),
            lint: Vec::new(),
            test: Vec::new(),
            dev: Vec::new(),
        }
    }

    fn touch_dir(root: &Path, rel: &str) {
        fs::create_dir_all(root.join(rel)).unwrap();
    }

    fn touch_file(root: &Path, rel: &str, contents: &str) {
        if let Some(parent) = root.join(rel).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(root.join(rel), contents).unwrap();
    }

    #[test]
    fn empty_tree_has_nothing_to_clean() {
        let root = temp("empty");
        let found = collect_dirs(&root, &[]);
        assert!(found.is_empty(), "{found:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn includes_erno_dir_at_the_project_root() {
        let root = temp("erno-dir");
        touch_dir(&root, ".erno");
        assert_eq!(collect_dirs(&root, &[]), vec![".erno"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn includes_known_artifacts_when_they_exist() {
        let root = temp("artifacts");
        for rel in [
            "api/target",
            "app/node_modules",
            "app/dist",
            "app/.angular",
            "www/.astro",
            "e2e/test-results",
        ] {
            touch_dir(&root, rel);
        }
        assert_eq!(
            collect_dirs(&root, &[]),
            vec![
                "api/target",
                "app/.angular",
                "app/dist",
                "app/node_modules",
                "e2e/test-results",
                "www/.astro",
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn does_not_collect_source_or_config() {
        let root = temp("source");
        touch_file(&root, "api/src/lib.rs", "fn main() {}");
        touch_file(&root, ".env", "SECRET=1");
        touch_file(&root, "config/local.toml", "");
        touch_file(&root, "api/config/development.toml", "[database]\n");
        touch_dir(&root, "api/src");
        assert!(collect_dirs(&root, &[pkg("api", "api")]).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn includes_opt_in_package_dirs_from_the_manifest() {
        let root = temp("opt-in");
        touch_dir(&root, "vision/target");
        let mut vision = pkg("vision", "vision");
        vision.default = false;
        assert_eq!(
            collect_dirs(&root, &[vision]),
            vec!["vision/target"]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn includes_admin_even_when_the_manifest_omits_it() {
        let root = temp("admin");
        touch_dir(&root, "admin/node_modules");
        touch_dir(&root, "admin/dist");
        assert_eq!(
            collect_dirs(&root, &[pkg("api", "api")]),
            vec!["admin/dist", "admin/node_modules"]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn names_are_project_relative_and_sorted() {
        let root = temp("sorted");
        touch_dir(&root, ".erno");
        touch_dir(&root, "www/dist");
        touch_dir(&root, "api/target");
        assert_eq!(
            collect_dirs(&root, &[]),
            vec![".erno", "api/target", "www/dist"]
        );
        let _ = fs::remove_dir_all(&root);
    }
}
