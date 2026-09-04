//! Local PostgreSQL helpers shared by `doctor`, `dev`, `clean`, and `test`.

use std::env;
use std::path::PathBuf;
use std::process::Command;

use tokio_postgres::config::Host;
use tokio_postgres::Config as PgConfig;

use crate::global_config::GlobalConfig;

/// One-line hint for starting a local PostgreSQL that isn't running.
///
/// `sudo service postgresql start` is a Debian sysvinit wrapper and is not
/// installed on Arch, Omarchy, Fedora, or other systemd-only hosts. The
/// command follows the host: systemd, Homebrew, or the Windows service
/// manager.
pub fn start_hint() -> String {
    match start_command_for(host_kind(), binaries_on_path()) {
        Some(cmd) => format!("Start it — e.g.: {cmd}"),
        None => "Start your PostgreSQL server.".to_string(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // all three exist so tests can cover every host
enum HostKind {
    Linux,
    Macos,
    Windows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Binaries {
    systemctl: bool,
    service: bool,
}

fn host_kind() -> HostKind {
    #[cfg(target_os = "macos")]
    {
        HostKind::Macos
    }
    #[cfg(target_os = "windows")]
    {
        HostKind::Windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        HostKind::Linux
    }
}

fn binaries_on_path() -> Binaries {
    Binaries {
        systemctl: on_path("systemctl"),
        service: on_path("service"),
    }
}

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| {
            env::split_paths(&paths).any(|dir| {
                let candidate: PathBuf = dir.join(name);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

/// `pg_isready` aimed at the server Erno actually uses.
///
/// Bare `pg_isready` talks to the unix socket in `/run/postgresql`. Erno's
/// admin URL is TCP (`localhost:5432`). Those are different endpoints — a
/// leftover server can accept TCP while the socket directory does not exist,
/// and doctor then reports "not running" next to a passing admin check.
pub fn pg_isready() -> Command {
    let url = GlobalConfig::load().ok().map(|c| c.postgres.admin_url);
    pg_isready_for(url.as_deref())
}

fn pg_isready_for(url: Option<&str>) -> Command {
    let mut cmd = Command::new("pg_isready");
    for arg in pg_isready_args(url) {
        cmd.arg(arg);
    }
    cmd
}

fn pg_isready_args(url: Option<&str>) -> Vec<String> {
    let (host, port) = url
        .and_then(endpoint_from_url)
        .unwrap_or_else(|| ("localhost".to_string(), 5432));
    vec!["-h".into(), host, "-p".into(), port.to_string()]
}

fn endpoint_from_url(url: &str) -> Option<(String, u16)> {
    let cfg: PgConfig = url.parse().ok()?;
    let host = match cfg.get_hosts().first()? {
        Host::Tcp(h) => h.clone(),
        #[cfg(unix)]
        Host::Unix(path) => path.to_string_lossy().into_owned(),
    };
    Some((host, cfg.get_ports().first().copied().unwrap_or(5432)))
}

fn start_command_for(host: HostKind, bins: Binaries) -> Option<&'static str> {
    match host {
        HostKind::Macos => Some("brew services start postgresql"),
        HostKind::Windows => Some("net start postgresql"),
        HostKind::Linux if bins.systemctl => Some("sudo systemctl start postgresql"),
        HostKind::Linux if bins.service => Some("sudo service postgresql start"),
        HostKind::Linux => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_linux_uses_systemctl() {
        assert_eq!(
            start_command_for(
                HostKind::Linux,
                Binaries {
                    systemctl: true,
                    service: true,
                }
            ),
            Some("sudo systemctl start postgresql")
        );
    }

    #[test]
    fn sysv_linux_keeps_service_when_that_is_what_exists() {
        assert_eq!(
            start_command_for(
                HostKind::Linux,
                Binaries {
                    systemctl: false,
                    service: true,
                }
            ),
            Some("sudo service postgresql start")
        );
    }

    #[test]
    fn linux_without_a_service_manager_is_honest() {
        assert_eq!(
            start_command_for(
                HostKind::Linux,
                Binaries {
                    systemctl: false,
                    service: false,
                }
            ),
            None
        );
    }

    #[test]
    fn macos_uses_homebrew() {
        assert_eq!(
            start_command_for(
                HostKind::Macos,
                Binaries {
                    systemctl: false,
                    service: false,
                }
            ),
            Some("brew services start postgresql")
        );
    }

    #[test]
    fn windows_uses_net_start() {
        assert_eq!(
            start_command_for(
                HostKind::Windows,
                Binaries {
                    systemctl: false,
                    service: false,
                }
            ),
            Some("net start postgresql")
        );
    }

    #[test]
    fn hint_on_this_host_is_a_complete_sentence() {
        let hint = start_hint();
        assert!(
            hint.starts_with("Start it — e.g.: ") || hint == "Start your PostgreSQL server.",
            "{hint}"
        );
        assert!(
            !hint.contains("Start it — e.g.: start "),
            "generic fallback must not be prefixed as a command: {hint}"
        );
    }

    #[test]
    fn systemd_linux_hosts_are_not_told_to_run_service() {
        if host_kind() != HostKind::Linux || !on_path("systemctl") {
            return;
        }
        assert_eq!(
            start_hint(),
            "Start it — e.g.: sudo systemctl start postgresql"
        );
    }

    #[test]
    fn erno_default_url_is_tcp_localhost() {
        assert_eq!(
            endpoint_from_url("postgres://erno:erno@localhost:5432/postgres"),
            Some(("localhost".into(), 5432))
        );
    }

    #[test]
    fn missing_port_defaults_to_5432() {
        assert_eq!(
            endpoint_from_url("postgres://erno:erno@localhost/postgres"),
            Some(("localhost".into(), 5432))
        );
    }

    #[test]
    fn custom_port_and_host_are_kept() {
        assert_eq!(
            endpoint_from_url("postgresql://erno@127.0.0.1:6543/postgres"),
            Some(("127.0.0.1".into(), 6543))
        );
    }

    #[test]
    fn invalid_url_is_none() {
        assert_eq!(endpoint_from_url("not a url"), None);
        assert_eq!(endpoint_from_url(""), None);
    }

    #[test]
    fn probe_args_fall_back_to_localhost_when_unconfigured() {
        assert_eq!(pg_isready_args(None), ["-h", "localhost", "-p", "5432"]);
        assert_eq!(
            pg_isready_args(Some("bogus")),
            ["-h", "localhost", "-p", "5432"]
        );
    }

    #[test]
    fn probe_args_follow_the_admin_url() {
        assert_eq!(
            pg_isready_args(Some("postgres://erno:erno@localhost:5432/postgres")),
            ["-h", "localhost", "-p", "5432"]
        );
        assert_eq!(
            pg_isready_args(Some("postgres://erno@db.internal:6543/postgres")),
            ["-h", "db.internal", "-p", "6543"]
        );
    }
}
