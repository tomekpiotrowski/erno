use std::path::PathBuf;
use std::process::Command;

pub fn find_ng_binary() -> Option<PathBuf> {
    if Command::new("ng")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Some(PathBuf::from("ng"));
    }

    let extra_dirs: Vec<PathBuf> = [
        dirs::home_dir().map(|h| h.join(".npm-global/bin")),
        Some(PathBuf::from("/usr/local/bin")),
        Some(PathBuf::from("/usr/bin")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for dir in extra_dirs {
        let candidate = dir.join("ng");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Some(nvm_dir) = dirs::home_dir().map(|h| h.join(".local/share/nvm")) {
        if let Ok(entries) = std::fs::read_dir(&nvm_dir) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("bin/ng");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

pub fn find_ionic_binary() -> Option<PathBuf> {
    if Command::new("ionic")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Some(PathBuf::from("ionic"));
    }

    let extra_dirs: Vec<PathBuf> = [
        dirs::home_dir().map(|h| h.join(".npm-global/bin")),
        dirs::home_dir().map(|h| h.join(".bun/bin")),
        Some(PathBuf::from("/usr/local/bin")),
        Some(PathBuf::from("/usr/bin")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for dir in extra_dirs {
        let candidate = dir.join("ionic");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Some(nvm_dir) = dirs::home_dir().map(|h| h.join(".local/share/nvm")) {
        if let Ok(entries) = std::fs::read_dir(&nvm_dir) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("bin/ionic");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}
