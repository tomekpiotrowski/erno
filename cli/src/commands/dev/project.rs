use std::path::{Path, PathBuf};

/// Walk `start` and its parents looking for an Erno project (`erno.toml`, or an
/// `api/Cargo.toml` for projects that have not declared a manifest).
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
    dir.join("erno.toml").is_file() || dir.join("api").join("Cargo.toml").is_file()
}

pub fn resolve_project_root(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    let start = match explicit {
        Some(path) => path,
        None => {
            std::env::current_dir().map_err(|e| format!("cannot read current directory: {e}"))?
        }
    };
    find_project_root(&start).ok_or_else(|| {
        format!(
            "no Erno project found\n\
             Looked for erno.toml or api/Cargo.toml from {}.\n\
             Run this inside a project, or create one with `erno new`.",
            start.display()
        )
    })
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
    fn requires_a_manifest_or_api_cargo_toml() {
        let tmp = temp_tree("no-cargo");
        fs::create_dir_all(tmp.join("api")).unwrap();
        assert!(find_project_root(&tmp).is_none());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn erno_toml_alone_marks_the_root() {
        let tmp = temp_tree("manifest-only");
        let nested = tmp.join("proj/puzzles/src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            tmp.join("proj/erno.toml"),
            "[[package]]\nname = \"x\"\ndir = \"x\"\n",
        )
        .unwrap();
        assert_eq!(find_project_root(&nested).unwrap(), tmp.join("proj"));
        let _ = fs::remove_dir_all(&tmp);
    }
}
