//! Exercise dependency installation through the real command-line entry point.

#[cfg(unix)]
mod tests {
    use lets_expect::lets_expect;
    use std::env::temp_dir;
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt;
    use std::process::{id, Command};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn build(
        bun_present: bool,
        installed: bool,
        install_status: u8,
        build_status: u8,
    ) -> (bool, Vec<String>) {
        let root = temp_dir().join(format!(
            "erno-bun-command-{}-{}",
            id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let app = root.join("app");
        let bin = root.join("bin");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join("api/Cargo.toml"), "[package]\nname = 'test'\n").unwrap();
        fs::write(app.join("package.json"), r#"{"scripts":{"build":"tool"}}"#).unwrap();
        if installed {
            fs::create_dir(app.join("node_modules")).unwrap();
        }
        if bun_present {
            fs::write(
                bin.join("bun"),
                r#"#!/bin/sh
printf '%s|%s|%s\n' "$PWD" "$*" "${BUN_FEATURE_FLAG_DISABLE_STREAMING_INSTALL:-}" >> "$CALLS"
if [ "$1" = install ]; then exit "$INSTALL_STATUS"; fi
exit "$BUILD_STATUS"
"#,
            )
            .unwrap();
            fs::set_permissions(bin.join("bun"), Permissions::from_mode(0o755)).unwrap();
        }
        // An available npm must never become a fallback.
        fs::write(
            bin.join("npm"),
            "#!/bin/sh\nprintf 'npm fallback\\n' >> \"$CALLS\"\nexit 0\n",
        )
        .unwrap();
        fs::set_permissions(bin.join("npm"), Permissions::from_mode(0o755)).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_erno"))
            .args(["--no-color", "build", "--app"])
            .current_dir(&root)
            .env("PATH", &bin)
            .env("CALLS", root.join("calls"))
            .env("INSTALL_STATUS", install_status.to_string())
            .env("BUILD_STATUS", build_status.to_string())
            .env_remove("BUN_FEATURE_FLAG_DISABLE_STREAMING_INSTALL")
            .output()
            .unwrap();
        let calls = fs::read_to_string(root.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(|line| line.replace(app.to_str().unwrap(), "app"))
            .collect();
        fs::remove_dir_all(root).unwrap();
        (output.status.success(), calls)
    }

    lets_expect! {
        expect(build(true, false, 0, 0)) as fresh_install
            to equal((true, vec!["app|install|1".into(), "app|run build|".into()]))
        expect(build(true, true, 0, 0)) as existing_dependencies
            to equal((true, vec!["app|run build|".into()]))
        expect(build(true, false, 7, 0)) as failed_install
            to equal((false, vec!["app|install|1".into()]))
        expect(build(true, true, 0, 7)) as failed_build
            to equal((false, vec!["app|run build|".into()]))
        expect(build(false, false, 0, 0)) as missing_bun
            to equal((false, Vec::<String>::new()))
    }
}
