use std::path::{Path, PathBuf};

/// Walk `start` and its parents looking for an Erno project (`api/Cargo.toml`).
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if is_project_root(&dir) {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub fn is_project_root(dir: &Path) -> bool {
    dir.join("api").join("Cargo.toml").is_file()
}

pub fn resolve_project_root(explicit: Option<PathBuf>) -> PathBuf {
    let start =
        explicit.unwrap_or_else(|| std::env::current_dir().expect("cannot read current directory"));
    match find_project_root(&start) {
        Some(root) => root,
        None => {
            eprintln!(
                "No Erno project found (looked for api/Cargo.toml from {}).",
                start.display()
            );
            eprintln!("Run `erno dev` inside a project, or create one with `erno new`.");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_tree(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "erno-dev-root-{}-{}-{suffix}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn finds_root_from_nested_directory() {
        let tmp = temp_tree("nested");
        let src = tmp.join("proj/api/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(tmp.join("proj/api/Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::create_dir_all(tmp.join("proj/app")).unwrap();

        let found = find_project_root(&src).unwrap();
        assert_eq!(found, tmp.join("proj"));
        assert_eq!(
            find_project_root(&tmp.join("proj")).unwrap(),
            tmp.join("proj")
        );
        assert!(find_project_root(&tmp).is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn requires_api_cargo_toml() {
        let tmp = temp_tree("no-cargo");
        fs::create_dir_all(tmp.join("api")).unwrap();
        assert!(find_project_root(&tmp).is_none());
        let _ = fs::remove_dir_all(&tmp);
    }
}
