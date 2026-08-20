use std::net::IpAddr;
use std::path::{Path, PathBuf};

use super::ports::port_from_url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePlatform {
    Ios,
    Android,
}

impl DevicePlatform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

/// How to invoke the Ionic CLI for device live-reload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IonicCli {
    pub program: PathBuf,
    /// Arguments that precede `cap run …` — empty unless we fall back to npx.
    pub prefix: Vec<String>,
}

/// The app's own `node_modules/.bin/ionic` first, then a CLI on PATH, then npx.
///
/// The npx form must name `@ionic/cli` and pass `--yes`: bare `ionic` resolves
/// to the deprecated 2019 package, and without `--yes` npx blocks forever on
/// "Ok to proceed?" with the prompt buried in the child's piped stdout.
pub fn resolve_ionic(app_dir: &Path) -> IonicCli {
    let local = app_dir.join("node_modules/.bin/ionic");
    if local.is_file() {
        return IonicCli {
            program: local,
            prefix: Vec::new(),
        };
    }
    if let Some(found) = crate::ng::find_ionic_binary() {
        return IonicCli {
            program: found,
            prefix: Vec::new(),
        };
    }
    IonicCli {
        program: PathBuf::from("npx"),
        prefix: vec!["--yes".to_string(), "@ionic/cli".to_string()],
    }
}

impl IonicCli {
    pub fn is_npx(&self) -> bool {
        self.program == Path::new("npx")
    }

    /// The full argument vector for a live-reload run on `platform`.
    ///
    /// `--no-interactive` is not optional: the child runs in its own process
    /// group with piped stdio, so any prompt it opens would take SIGTTIN and
    /// stop the process — a hang with nothing on screen. The target is resolved
    /// here instead, by `choose_target`.
    pub fn cap_run_args(&self, platform: DevicePlatform, port: u16, target: &str) -> Vec<String> {
        let mut args = self.prefix.clone();
        args.extend(
            [
                "cap",
                "run",
                platform.as_str(),
                "--livereload",
                "--external",
                "--port",
            ]
            .map(String::from),
        );
        args.push(port.to_string());
        args.push("--target".to_string());
        args.push(target.to_string());
        args.push("--no-interactive".to_string());
        args
    }
}

/// A device or emulator reported by `cap run <platform> --list --json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub name: String,
}

pub fn parse_targets(json: &str) -> Vec<Target> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str(json) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_string();
            let name = item
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or(&id)
                .to_string();
            Some(Target { id, name })
        })
        .collect()
}

/// Ask the Capacitor CLI which devices and emulators are attached.
pub fn list_targets(app_dir: &Path, platform: DevicePlatform) -> Result<Vec<Target>, String> {
    let local = app_dir.join("node_modules/.bin/cap");
    let mut cmd = if local.is_file() {
        std::process::Command::new(local)
    } else {
        let mut cmd = std::process::Command::new("npx");
        cmd.args(["--yes", "@capacitor/cli"]);
        cmd
    };
    let output = cmd
        .args(["run", platform.as_str(), "--list", "--json"])
        .current_dir(app_dir)
        .output()
        .map_err(|e| format!("could not run the Capacitor CLI in app/: {e}"))?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        let details = if details.trim().is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            details.trim().to_string()
        };
        return Err(format!(
            "could not list {} targets\n{details}",
            platform.as_str()
        ));
    }
    Ok(parse_targets(&String::from_utf8_lossy(&output.stdout)))
}

/// One target is used as-is; anything else needs the user to say which.
pub fn choose_target(targets: &[Target], platform: DevicePlatform) -> Result<String, String> {
    let platform = platform.as_str();
    match targets {
        [] => Err(format!(
            "no {platform} device or emulator is available\n\
             Connect a device (or start an emulator) and try again."
        )),
        [only] => Ok(only.id.clone()),
        many => {
            let width = crate::ui::column_width(many.iter().map(|t| t.id.as_str()));
            let list = many
                .iter()
                .map(|t| format!("{:width$}  {}", t.id, t.name))
                .collect::<Vec<_>>()
                .join("\n");
            Err(format!(
                "several {platform} targets are available — pick one with --target <id>\n{list}"
            ))
        }
    }
}

/// `ionic cap run` needs the native project to exist before it can build it.
pub fn ensure_platform_added(app_dir: &Path, platform: DevicePlatform) -> Result<(), String> {
    if app_dir.join(platform.as_str()).is_dir() {
        return Ok(());
    }
    Err(format!(
        "app/{platform} does not exist\n\
         Add the native project first: cd app && npx cap add {platform}",
        platform = platform.as_str()
    ))
}

/// Best-effort LAN address: UDP connect does not send packets.
pub fn lan_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip)
    }
}

pub fn rewrite_url_host(url: &str, ip: IpAddr) -> String {
    let port = port_from_url(Some(url)).unwrap_or(80);
    let scheme = url.split("://").next().unwrap_or("http");
    format!("{scheme}://{ip}:{port}")
}

pub fn cors_origins(lan_app: &str) -> String {
    [
        lan_app,
        "http://localhost:4200",
        "capacitor://localhost",
        "ionic://localhost",
        "http://localhost",
        "https://localhost",
    ]
    .join(",")
}

/// Restore the previous app URL file when `erno dev` exits.
pub struct UrlRewrite {
    path: PathBuf,
    original: String,
}

impl Drop for UrlRewrite {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.path, &self.original);
    }
}

pub fn apply_lan_api_urls(
    app_dir: &Path,
    api_http: &str,
    api_ws: &str,
) -> Result<UrlRewrite, String> {
    let env_path = app_dir.join("src/environments/environment.ts");
    let main_path = app_dir.join("src/main.ts");
    let module_path = app_dir.join("src/app/app.module.ts");
    let path = if env_path.is_file() {
        env_path
    } else if main_path.is_file() {
        main_path
    } else if module_path.is_file() {
        module_path
    } else {
        return Err(format!(
            "cannot find src/environments/environment.ts, src/main.ts, or src/app/app.module.ts to point the app at {api_http}"
        ));
    };
    let original = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let next = rewrite_source(&original, api_http, api_ws);
    std::fs::write(&path, &next).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(UrlRewrite { path, original })
}

pub fn rewrite_source(source: &str, api_http: &str, api_ws: &str) -> String {
    source
        .replace("http://localhost:3000", api_http)
        .replace("ws://localhost:3000", api_ws)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_localhost_host() {
        let ip: IpAddr = "192.168.1.20".parse().unwrap();
        assert_eq!(
            rewrite_url_host("http://localhost:3000", ip),
            "http://192.168.1.20:3000"
        );
        assert_eq!(
            rewrite_url_host("ws://localhost:3000", ip),
            "ws://192.168.1.20:3000"
        );
    }

    #[test]
    fn rewrites_module_literals() {
        let src = "baseUrl: 'http://localhost:3000', wsUrl: 'ws://localhost:3000'";
        let out = rewrite_source(src, "http://10.0.0.2:3000", "ws://10.0.0.2:3000");
        assert!(out.contains("http://10.0.0.2:3000"));
        assert!(out.contains("ws://10.0.0.2:3000"));
        assert!(!out.contains("localhost:3000"));
    }

    #[test]
    fn npx_fallback_names_the_current_package_and_skips_the_prompt() {
        let cli = IonicCli {
            program: PathBuf::from("npx"),
            prefix: vec!["--yes".to_string(), "@ionic/cli".to_string()],
        };
        assert!(cli.is_npx());
        let args = cli.cap_run_args(DevicePlatform::Android, 4200, "emulator-5554");
        assert_eq!(
            args,
            [
                "--yes",
                "@ionic/cli",
                "cap",
                "run",
                "android",
                "--livereload",
                "--external",
                "--port",
                "4200",
                "--target",
                "emulator-5554",
                "--no-interactive",
            ]
        );
    }

    #[test]
    fn a_resolved_binary_runs_cap_run_directly() {
        let cli = IonicCli {
            program: PathBuf::from("/usr/local/bin/ionic"),
            prefix: Vec::new(),
        };
        assert!(!cli.is_npx());
        assert_eq!(
            cli.cap_run_args(DevicePlatform::Ios, 4300, "ABC-123"),
            [
                "cap",
                "run",
                "ios",
                "--livereload",
                "--external",
                "--port",
                "4300",
                "--target",
                "ABC-123",
                "--no-interactive",
            ]
        );
    }

    #[test]
    fn parses_capacitor_target_json() {
        let json = r#"[{"name":"Pixel 8","api":"14","id":"emulator-5554"},{"id":"bare-id"}]"#;
        let targets = parse_targets(json);
        assert_eq!(
            targets,
            [
                Target {
                    id: "emulator-5554".to_string(),
                    name: "Pixel 8".to_string()
                },
                Target {
                    id: "bare-id".to_string(),
                    name: "bare-id".to_string()
                },
            ]
        );
        assert!(parse_targets("not json").is_empty());
    }

    #[test]
    fn a_single_target_is_chosen_and_ambiguity_is_reported() {
        let one = [Target {
            id: "emulator-5554".to_string(),
            name: "Pixel 8".to_string(),
        }];
        assert_eq!(
            choose_target(&one, DevicePlatform::Android).unwrap(),
            "emulator-5554"
        );

        let none = choose_target(&[], DevicePlatform::Ios).unwrap_err();
        assert!(none.contains("no ios device or emulator"));

        let mut many = one.to_vec();
        many.push(Target {
            id: "device-2".to_string(),
            name: "Phone".to_string(),
        });
        let err = choose_target(&many, DevicePlatform::Android).unwrap_err();
        assert!(err.contains("--target <id>"));
        assert!(err.contains("device-2"));
    }

    #[test]
    fn local_node_modules_bin_wins() {
        let dir = std::env::temp_dir().join("erno-dev-ionic-test");
        let bin = dir.join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("ionic"), "#!/bin/sh\n").unwrap();
        let cli = resolve_ionic(&dir);
        assert_eq!(cli.program, bin.join("ionic"));
        assert!(cli.prefix.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cors_list_includes_capacitor() {
        let list = cors_origins("http://192.168.1.5:4200");
        assert!(list.contains("capacitor://localhost"));
        assert!(list.contains("http://192.168.1.5:4200"));
    }
}
