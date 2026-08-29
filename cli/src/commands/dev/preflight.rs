use std::io::IsTerminal;
use std::process::Command;

use super::selection::ExtraService;
use crate::ui;

const FRIENDLY_COMMANDS: &[&str] = &[
    "erno",
    "cargo",
    "node",
    "npm",
    "ng",
    "astro",
    "esbuild",
    "vite",
    "python",
    "python3",
    "prometheus",
    "tempo",
    "loki",
];

// The three telemetry binaries above: `erno dev` no longer starts them, but
// they stay auto-killable when they squat a port it wants. A stale Tempo left
// behind by the monitoring app is exactly the case worth offering to clear.

pub fn run_preflight(check_db: bool, extras: &[ExtraService], ports: &[u16]) -> Result<(), String> {
    if check_db {
        check_postgres()?;
    }
    for extra in extras {
        if let Some(binary) = extra.requires.as_deref() {
            check_required_binary(&extra.name, binary)?;
        }
    }
    for port in ports {
        check_port(*port)?;
    }
    Ok(())
}

/// A binary a declared service says it needs.
///
/// Checked before anything is spawned, because the alternative is a supervisor
/// restarting a command that will never exist, with the same spawn error
/// scrolling past and nothing naming what to install.
fn check_required_binary(service: &str, binary: &str) -> Result<(), String> {
    if binary_on_path(binary) {
        return Ok(());
    }
    Err(format!(
        "{binary} not found on PATH, and `{service}` needs it\n\
         Install it, or drop that [[package.dev]] from erno.toml."
    ))
}

fn binary_on_path(binary: &str) -> bool {
    Command::new(binary)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        || Command::new(binary)
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success())
}

fn check_postgres() -> Result<(), String> {
    match Command::new("pg_isready").output() {
        Err(_) => Err("PostgreSQL client tools not found (`pg_isready`)\n\
                       Install PostgreSQL: https://www.postgresql.org/download/"
            .to_string()),
        Ok(out) if !out.status.success() => Err("PostgreSQL is not running\n\
                       Start it — e.g.: sudo service postgresql start"
            .to_string()),
        Ok(_) => Ok(()),
    }
}

fn check_port(port: u16) -> Result<(), String> {
    if !port_in_use(port) {
        return Ok(());
    }

    let pid = pid_listening_on(port);
    let comm = pid
        .and_then(process_name)
        .unwrap_or_else(|| "unknown".into());
    let pid_label = pid.map(|p| p.to_string()).unwrap_or_else(|| "?".into());

    ui::warn(format!("port {port} is in use by pid {pid_label} ({comm})"));

    if !std::io::stdin().is_terminal() {
        return Err(format!(
            "port {port} is in use\n\
             Free it, or re-run from a terminal to be prompted to kill pid {pid_label}."
        ));
    }

    if !ui::confirm("Kill it?", should_default_kill(&comm)) {
        return Err("aborted".to_string());
    }

    let pid = pid.ok_or_else(|| {
        format!("could not identify the process holding port {port}\nFree it and try again.")
    })?;
    if !kill_pid(pid) {
        return Err(format!(
            "could not stop pid {pid}\nFree port {port} and try again."
        ));
    }
    // Brief wait for the socket to be released.
    std::thread::sleep(std::time::Duration::from_millis(300));
    if port_in_use(port) {
        return Err(format!(
            "port {port} is still in use after killing pid {pid}"
        ));
    }
    ui::ok(format!("Freed port {port}"));
    Ok(())
}

pub fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

pub fn should_default_kill(comm: &str) -> bool {
    let name = comm.rsplit('/').next().unwrap_or(comm);
    FRIENDLY_COMMANDS
        .iter()
        .any(|c| name == *c || name.starts_with(c))
}

fn pid_listening_on(port: u16) -> Option<u32> {
    if let Ok(out) = Command::new("lsof")
        .args(["-t", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output()
    {
        if let Some(pid) = parse_lsof_pid(&String::from_utf8_lossy(&out.stdout)) {
            return Some(pid);
        }
    }

    let fuser = Command::new("fuser")
        .arg(format!("{port}/tcp"))
        .output()
        .ok()?;
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&fuser.stdout),
        String::from_utf8_lossy(&fuser.stderr)
    );
    parse_fuser_pid(&text)
}

/// `lsof -t` prints one PID per line.
pub fn parse_lsof_pid(stdout: &str) -> Option<u32> {
    stdout.split_whitespace().find_map(|s| s.parse().ok())
}

/// `fuser` prints `3000/tcp:           12345` (port first, PID after the colon).
pub fn parse_fuser_pid(text: &str) -> Option<u32> {
    text.split(':')
        .nth(1)?
        .split_whitespace()
        .find_map(|s| s.parse().ok())
}

fn process_name(pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn kill_pid(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let term = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        if term != 0 {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        // ESRCH means it already exited.
        let again = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if again == 0 {
            let _ = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
        true
    }
    #[cfg(not(unix))]
    {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(name: &str, requires: Option<&str>) -> ExtraService {
        ExtraService {
            name: name.into(),
            dir: name.into(),
            command: name.into(),
            args: vec![],
            url: "http://localhost:1".into(),
            requires: requires.map(str::to_string),
            ports: vec![],
        }
    }

    /// A declared service names the binary it needs, so a missing one is one
    /// line rather than a supervisor restarting a command that will never
    /// exist.
    #[test]
    fn a_declared_service_names_the_binary_it_is_missing() {
        let err = run_preflight(
            false,
            &[service("tempo", Some("definitely-not-a-real-binary"))],
            &[],
        )
        .expect_err("should refuse");
        assert!(err.contains("definitely-not-a-real-binary"), "{err}");
        assert!(err.contains("tempo"), "names the service too: {err}");
    }

    #[test]
    fn a_service_that_requires_nothing_is_not_checked() {
        run_preflight(false, &[service("www", None)], &[]).expect("nothing to check");
    }

    #[test]
    fn default_kill_for_dev_tools_only() {
        assert!(should_default_kill("cargo"));
        assert!(should_default_kill("node"));
        assert!(should_default_kill("/usr/bin/npm"));
        assert!(should_default_kill("ng"));
        assert!(should_default_kill("python3"));
        assert!(should_default_kill("/usr/bin/python"));
        assert!(should_default_kill("tempo"));
        assert!(should_default_kill("loki"));
        assert!(!should_default_kill("firefox"));
        assert!(!should_default_kill("postgres"));
    }

    /// Retried, because the freed port is an ephemeral one the OS is free to
    /// hand to anything else on the machine the moment we let go of it — which
    /// it occasionally does, mid-suite, and failed the build.
    #[test]
    fn port_in_use_detects_bound_listener() {
        for attempt in 0..5 {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            assert!(port_in_use(port));
            drop(listener);
            if !port_in_use(port) {
                return;
            }
            assert!(attempt < 4, "every freed port was taken again");
        }
    }

    #[test]
    fn parses_lsof_and_fuser_output() {
        assert_eq!(parse_lsof_pid("12345\n"), Some(12345));
        assert_eq!(parse_fuser_pid("3000/tcp:           999\n"), Some(999));
        assert_eq!(parse_fuser_pid("nothing"), None);
    }
}
