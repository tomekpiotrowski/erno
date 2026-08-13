use std::io::{self, IsTerminal, Write};
use std::process::Command;

const FRIENDLY_COMMANDS: &[&str] = &[
    "erno", "cargo", "node", "npm", "ng", "astro", "esbuild", "vite",
];

pub fn run_preflight(check_db: bool, check_prometheus: bool, ports: &[u16]) {
    if check_db {
        check_postgres();
    }
    if check_prometheus {
        check_prometheus_binary();
    }
    for port in ports {
        check_port(*port);
    }
}

fn check_postgres() {
    match Command::new("pg_isready").output() {
        Err(_) => {
            eprintln!("PostgreSQL client tools not found (`pg_isready`).");
            eprintln!("Install PostgreSQL: https://www.postgresql.org/download/");
            std::process::exit(1);
        }
        Ok(out) if !out.status.success() => {
            eprintln!("PostgreSQL is not running.");
            eprintln!("Start it — e.g.: sudo service postgresql start");
            std::process::exit(1);
        }
        Ok(_) => {}
    }
}

fn check_prometheus_binary() {
    if super::prometheus::binary_on_path() {
        return;
    }
    eprintln!("prometheus not found on PATH.");
    eprintln!("Install Prometheus: https://prometheus.io/docs/prometheus/latest/installation/");
    eprintln!("Or pass --no-prometheus to start without the scrape server.");
    std::process::exit(1);
}

fn check_port(port: u16) {
    if !port_in_use(port) {
        return;
    }

    let pid = pid_listening_on(port);
    let comm = pid
        .and_then(process_name)
        .unwrap_or_else(|| "unknown".into());
    let pid_label = pid.map(|p| p.to_string()).unwrap_or_else(|| "?".into());

    eprintln!("Port {port} is in use by pid {pid_label} ({comm}).");

    if !io::stdin().is_terminal() {
        eprintln!("Free the port or re-run `erno dev` from a terminal to be prompted.");
        std::process::exit(1);
    }

    let default_yes = should_default_kill(&comm);
    let prompt = if default_yes { "[Y/n]" } else { "[y/N]" };
    if !confirm(&format!("Kill it? {prompt} "), default_yes) {
        eprintln!("Aborting.");
        std::process::exit(1);
    }

    match pid {
        Some(pid) => {
            if !kill_pid(pid) {
                eprintln!("Could not stop pid {pid}. Free port {port} and try again.");
                std::process::exit(1);
            }
            // Brief wait for the socket to be released.
            std::thread::sleep(std::time::Duration::from_millis(300));
            if port_in_use(port) {
                eprintln!("Port {port} is still in use after killing pid {pid}.");
                std::process::exit(1);
            }
            eprintln!("Freed port {port}.");
        }
        None => {
            eprintln!("Could not identify the process. Free port {port} and try again.");
            std::process::exit(1);
        }
    }
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

fn confirm(prompt: &str, default_yes: bool) -> bool {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return default_yes;
    }
    match input.trim().to_ascii_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default_yes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_kill_for_dev_tools_only() {
        assert!(should_default_kill("cargo"));
        assert!(should_default_kill("node"));
        assert!(should_default_kill("/usr/bin/npm"));
        assert!(should_default_kill("ng"));
        assert!(!should_default_kill("firefox"));
        assert!(!should_default_kill("postgres"));
    }

    #[test]
    fn port_in_use_detects_bound_listener() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_in_use(port));
        drop(listener);
        assert!(!port_in_use(port));
    }

    #[test]
    fn parses_lsof_and_fuser_output() {
        assert_eq!(parse_lsof_pid("12345\n"), Some(12345));
        assert_eq!(parse_fuser_pid("3000/tcp:           999\n"), Some(999));
        assert_eq!(parse_fuser_pid("nothing"), None);
    }
}
