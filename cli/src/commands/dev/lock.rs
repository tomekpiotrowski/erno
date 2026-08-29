use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct DevLock {
    path: PathBuf,
}

impl DevLock {
    pub fn acquire(root: &Path) -> Result<Self, String> {
        let dir = root.join(".erno");
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let path = dir.join("dev.lock");

        if path.exists() {
            if let Some(pid) = read_pid(&path) {
                if pid_is_alive(pid) {
                    return Err(format!(
                        "erno dev is already running (pid {pid}, {}).",
                        path.display()
                    ));
                }
            }
            let _ = fs::remove_file(&path);
        }

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        writeln!(file, "pid={}", std::process::id())
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for DevLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Pid of a live `erno dev` for this project, if one holds `.erno/dev.lock`.
pub fn running_pid(root: &Path) -> Option<u32> {
    let pid = read_pid(&root.join(".erno").join("dev.lock"))?;
    if pid_is_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

pub fn read_pid(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    parse_pid(&text)
}

pub fn parse_pid(text: &str) -> Option<u32> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("pid=") {
            return rest.trim().parse().ok();
        }
        if let Ok(pid) = line.parse::<u32>() {
            return Some(pid);
        }
    }
    None
}

pub fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pid_line() {
        assert_eq!(parse_pid("pid=12345\n"), Some(12345));
        assert_eq!(parse_pid("12345\n"), Some(12345));
        assert_eq!(parse_pid(""), None);
    }

    #[test]
    fn acquire_replaces_stale_lock() {
        let dir = std::env::temp_dir().join(format!(
            "erno-lock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join(".erno")).unwrap();
        fs::write(dir.join(".erno/dev.lock"), "pid=1\n").unwrap(); // pid 1 may be alive on linux!

        // Use a pid that is almost certainly dead.
        fs::write(dir.join(".erno/dev.lock"), "pid=999999\n").unwrap();
        {
            let lock = DevLock::acquire(&dir).expect("stale lock should be replaced");
            assert!(dir.join(".erno/dev.lock").exists());
            drop(lock);
        }
        assert!(!dir.join(".erno/dev.lock").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn current_process_is_alive() {
        assert!(pid_is_alive(std::process::id()));
        assert!(!pid_is_alive(0));
    }

    fn lock_tree(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "erno-lock-{}-{}-{suffix}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".erno")).unwrap();
        dir
    }

    #[test]
    fn running_pid_is_none_without_a_lock() {
        let dir = lock_tree("missing");
        assert_eq!(running_pid(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn running_pid_ignores_a_stale_lock() {
        let dir = lock_tree("stale");
        fs::write(dir.join(".erno/dev.lock"), "pid=999999\n").unwrap();
        assert_eq!(running_pid(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn running_pid_reports_a_live_process() {
        let dir = lock_tree("live");
        let pid = std::process::id();
        fs::write(dir.join(".erno/dev.lock"), format!("pid={pid}\n")).unwrap();
        assert_eq!(running_pid(&dir), Some(pid));
        let _ = fs::remove_dir_all(&dir);
    }
}
