//! Shared Bun installation policy for generated apps and local commands.

use std::fs;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

pub const PACKAGE_MANAGER: &str = "bun@1.4.0";

/// Install dependencies using Bun's non-streaming extractor.
///
/// Bun 1.4's streaming extraction can miss optional native packages such as
/// lightningcss-linux-x64-gnu. Every Erno-managed install uses this workaround.
pub fn install(dir: &Path) -> Command {
    let mut command = Command::new("bun");
    command
        .arg("install")
        .env("BUN_FEATURE_FLAG_DISABLE_STREAMING_INSTALL", "1")
        .current_dir(dir);
    command
}

/// Keep Angular's dependency operations on Bun, including `ng update`.
pub fn configure_angular(dir: &Path) -> Result<(), Error> {
    let path = dir.join("angular.json");
    let mut config: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    if !config.is_object() || !(config["cli"].is_null() || config["cli"].is_object()) {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "invalid Angular configuration",
        ));
    }
    if config["cli"].is_null() {
        config["cli"] = json!({});
    }
    config["cli"]["packageManager"] = json!("bun");
    fs::write(path, serde_json::to_string_pretty(&config)? + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lets_expect::lets_expect;
    use std::env::temp_dir;
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use std::process::id;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn configure(source: &str) -> (bool, String) {
        let dir: PathBuf = temp_dir().join(format!(
            "erno-bun-config-{}-{}",
            id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("angular.json"), source).unwrap();
        let success = configure_angular(&dir).is_ok();
        let result = fs::read_to_string(dir.join("angular.json")).unwrap();
        fs::remove_dir_all(dir).unwrap();
        (success, result)
    }

    lets_expect! {
        expect(install(Path::new("/tmp/an app"))) {
            to preserve_command_and_directory {
                have(get_program()) equal(OsStr::new("bun")),
                have(get_args().collect::<Vec<_>>()) equal(vec![OsStr::new("install")]),
                have(get_current_dir()) equal(Some(Path::new("/tmp/an app"))),
                have(get_envs().collect::<Vec<_>>()) equal(vec![(OsStr::new("BUN_FEATURE_FLAG_DISABLE_STREAMING_INSTALL"), Some(OsStr::new("1")))])
            }
        }
        expect(configure(source)) {
            when(source = "{}") as missing_cli to make_a_cli_section {
                have(0) be_true,
                have(1.contains("\"packageManager\": \"bun\"")) be_true
            }
            when(source = r#"{"cli":{"packageManager":"npm","analytics":false},"projects":{}}"#) as existing_cli to preserve_other_settings {
                have(0) be_true,
                have(1.contains("\"packageManager\": \"bun\"")) be_true,
                have(1.contains("\"analytics\": false")) be_true,
                have(1.contains("\"projects\": {}")) be_true
            }
            when(source = "invalid") as invalid_json to equal((false, source.into()))
            when(source = "[]") as invalid_root to equal((false, source.into()))
            when(source = r#"{"cli":false}"#) as invalid_cli to equal((false, source.into()))
        }
    }
}
