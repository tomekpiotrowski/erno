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
        if !Path::new("monitoring/Cargo.toml").exists() {
            ui::abort(
                "this project has no monitoring/\n\
                 It predates monitoring scaffolding. Copy monitoring/ from an erno\n\
                 checkout, or re-scaffold with `erno new --erno-path <path-to-erno>`.",
            );
        }
        if !Path::new("monitoring/ui/package.json").exists() {
            ui::abort("monitoring/ui is missing — the operator console cannot be built");
        }
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
