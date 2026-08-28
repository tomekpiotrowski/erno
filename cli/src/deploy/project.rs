use std::path::Path;

use super::Target;
use crate::ui;

pub fn validate_project_root(target: Target) {
    if !Path::new("api/Cargo.toml").exists() || !Path::new("app/package.json").exists() {
        ui::abort(
            "not an erno project root\n\
             Run this command from the directory that contains api/ and app/.",
        );
    }
    if target == Target::Monitoring {
        // Nothing else to check: the target was derived from this tree in the
        // first place, so it cannot disagree with it.
        return;
    }
    if !Path::new("www/package.json").exists() {
        ui::warn("no www/ marketing site found");
        ui::detail(
            "Newer scaffolds include www/ (Astro). Deploy still generates the www Docker files.\n\
             Add a www/ package, or set workloads.www = false in deploy/config.toml.",
        );
    }
}

/// Whether this tree is the collector rather than a product application.
pub fn is_collector_tree() -> bool {
    let config = std::fs::read_to_string("api/config/production.toml")
        .or_else(|_| std::fs::read_to_string("api/config/development.toml"))
        .unwrap_or_default();
    declares_collector(&config)
}

/// Whether an api config is the collector's.
///
/// `[collector]` and its subtables are the collector's alone — a product
/// application configures `[error_reporting]` to *send* to one, never a
/// `[collector]` to receive.
fn declares_collector(config: &str) -> bool {
    config
        .lines()
        .any(|l| l.trim_start().starts_with("[collector"))
}

pub fn read_project_name() -> String {
    let cargo_toml = std::fs::read_to_string("api/Cargo.toml")
        .unwrap_or_else(|_| ui::abort("could not read api/Cargo.toml"));

    for line in cargo_toml.lines() {
        if let Some(rest) = line.strip_prefix("name") {
            if let Some(name) = rest.trim().strip_prefix('=') {
                return name.trim().trim_matches('"').to_string();
            }
        }
    }

    ui::abort("could not parse the project name from api/Cargo.toml");
}

pub fn read_github_repo() -> String {
    let git_config = std::fs::read_to_string(".git/config").unwrap_or_default();
    let mut in_origin = false;
    for line in git_config.lines() {
        let trimmed = line.trim();
        if trimmed == "[remote \"origin\"]" {
            in_origin = true;
            continue;
        }
        if in_origin && trimmed.starts_with('[') {
            break;
        }
        if in_origin {
            if let Some(rest) = trimmed.strip_prefix("url") {
                if let Some(url) = rest.trim().strip_prefix('=') {
                    return extract_github_repo(url.trim());
                }
            }
        }
    }

    ui::abort(
        "could not detect the GitHub repo from .git/config remote origin\n\
         Ensure a GitHub remote is configured.",
    );
}

pub fn extract_github_repo(url: &str) -> String {
    let stripped = url.trim_end_matches(".git").trim_end_matches('/');

    if let Some(path) = stripped.strip_prefix("https://github.com/") {
        return path.to_string();
    }
    if let Some(path) = stripped.strip_prefix("git@github.com:") {
        return path.to_string();
    }

    ui::abort(&format!(
        "remote origin does not look like a GitHub URL: {url}"
    ));
}

pub fn www_present() -> bool {
    Path::new("www/package.json").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Target::detect` reads this, so a wrong answer here deploys the wrong
    /// chart into the wrong cluster — which is exactly what removing the
    /// `--target` flag was meant to make impossible.
    #[test]
    fn only_the_collector_declares_a_collector_table() {
        assert!(declares_collector("[collector]\nenabled = true\n"));
        assert!(declares_collector(
            "[server]\nport = 3001\n\n[collector.alerts]\n"
        ));
        assert!(declares_collector("  [collector]\n"));
        // A product app sends to a collector; it does not host one.
        assert!(!declares_collector(
            "[error_reporting]\ncollector_url = \"https://m.test\"\n"
        ));
        assert!(!declares_collector(""));
    }

    #[test]
    fn github_urls() {
        assert_eq!(
            extract_github_repo("https://github.com/acme/acme.git"),
            "acme/acme"
        );
        assert_eq!(
            extract_github_repo("git@github.com:acme/acme.git"),
            "acme/acme"
        );
    }
}
