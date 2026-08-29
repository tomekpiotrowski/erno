use std::path::Path;

use super::banner::DevUrls;
use super::selection::ServiceSelection;

pub fn discover_urls(root: &Path, sel: &ServiceSelection) -> DevUrls {
    let api_toml = std::fs::read_to_string(root.join("api/config/development.toml")).ok();
    let api_toml = api_toml.as_deref().unwrap_or("");

    let api_port = parse_table_u16(api_toml, "server", "port").unwrap_or(3000);
    let api_url = parse_assignment(api_toml, "api_url")
        .unwrap_or_else(|| format!("http://localhost:{api_port}"));

    let app_url_cfg = parse_assignment(api_toml, "app_url");
    let app_port = read_angular_port(root)
        .or_else(|| port_from_url(app_url_cfg.as_deref()))
        .unwrap_or(4200);
    let app_url = app_url_cfg.unwrap_or_else(|| format!("http://localhost:{app_port}"));

    let www_port = read_www_port(root).unwrap_or(4321);
    let www_url = format!("http://localhost:{www_port}");

    DevUrls {
        api: sel.api.then_some(api_url),
        app: sel.app.then_some(app_url),
        www: sel.www.then_some(www_url),
        // Filled in by `handle_dev`, but before the banner is pinned, since its
        // height is fixed at that point.
        admin: None,
        extra: Vec::new(),
    }
}

/// Every port `erno dev` is about to bind, including the extra ones a declared
/// service names.
///
/// A service can listen on more than one — a trace store answers queries on the
/// port in its `url` and receives on another — and one that came up without the
/// second bound looks healthy and accepts nothing.
pub fn ports_to_check(urls: &DevUrls, extras: &[super::selection::ExtraService]) -> Vec<u16> {
    let mut ports = Vec::new();
    if let Some(url) = &urls.api {
        ports.push(port_from_url(Some(url)).unwrap_or(3000));
    }
    if let Some(url) = &urls.app {
        ports.push(port_from_url(Some(url)).unwrap_or(4200));
    }
    if let Some(url) = &urls.www {
        ports.push(port_from_url(Some(url)).unwrap_or(4321));
    }
    if let Some(url) = &urls.admin {
        ports.push(port_from_url(Some(url)).unwrap_or(4300));
    }
    for (_, url) in &urls.extra {
        if let Some(port) = port_from_url(Some(url)) {
            ports.push(port);
        }
    }
    for extra in extras {
        ports.extend(&extra.ports);
    }
    ports
}

pub fn parse_table_string(toml: &str, table: &str, key: &str) -> Option<String> {
    let mut in_table = false;
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_table = line[1..line.len() - 1] == *table;
            continue;
        }
        if in_table {
            if let Some(val) = parse_assignment_line(line, key) {
                return Some(val);
            }
        }
    }
    None
}

pub fn parse_table_u16(toml: &str, table: &str, key: &str) -> Option<u16> {
    let mut in_table = false;
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_table = line[1..line.len() - 1] == *table;
            continue;
        }
        if in_table {
            if let Some(val) = parse_assignment_line(line, key) {
                return val.parse().ok();
            }
        }
    }
    None
}

pub fn parse_assignment(toml: &str, key: &str) -> Option<String> {
    for line in toml.lines() {
        if let Some(val) = parse_assignment_line(line.trim(), key) {
            return Some(val);
        }
    }
    None
}

fn parse_assignment_line(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim();
    let rest = rest.strip_prefix('=')?.trim();
    let val = rest.trim_matches('"').trim_matches('\'').trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

pub fn port_from_url(url: Option<&str>) -> Option<u16> {
    let url = url?;
    // http://host:3000 or http://host:3000/path
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let hostport = after_scheme.split('/').next()?;
    hostport.split(':').nth(1)?.parse().ok()
}

pub fn read_angular_port(root: &Path) -> Option<u16> {
    let content = std::fs::read_to_string(root.join("app/angular.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json["projects"]
        .as_object()?
        .values()
        .find_map(|proj| proj["architect"]["serve"]["options"]["port"].as_u64())
        .map(|p| p as u16)
}

pub fn read_www_port(root: &Path) -> Option<u16> {
    port_from_package_script(&root.join("www"), "dev")
}

/// The `--port` a package.json script passes, for the projects whose port is
/// declared there rather than in a framework config.
///
/// `www/` is an Astro site with no `angular.json` to read, so its `dev` script
/// is the only place its port is written down.
pub fn port_from_package_script(dir: &Path, script: &str) -> Option<u16> {
    let content = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    port_from_script(json["scripts"][script].as_str()?)
}

pub fn port_from_script(script: &str) -> Option<u16> {
    let mut parts = script.split_whitespace();
    while let Some(part) = parts.next() {
        if part == "--port" || part == "-p" {
            return parts.next()?.parse().ok();
        }
        if let Some(val) = part.strip_prefix("--port=") {
            return val.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_port_and_urls() {
        let toml = r#"
api_url = "http://localhost:3010"
app_url = "http://localhost:4210"

[server]
port = 3010
"#;
        assert_eq!(
            parse_table_string(
                "[database]\nurl = \"postgres://x:y@localhost/db\"\n",
                "database",
                "url"
            )
            .as_deref(),
            Some("postgres://x:y@localhost/db")
        );
        assert_eq!(parse_table_u16(toml, "server", "port"), Some(3010));
        assert_eq!(
            parse_assignment(toml, "api_url").as_deref(),
            Some("http://localhost:3010")
        );
        assert_eq!(port_from_url(Some("http://localhost:3010/")), Some(3010));
    }

    #[test]
    fn ignores_port_outside_server_table() {
        let toml = r#"
[jobs.cleanup]
port = 9

[server]
port = 3005
"#;
        assert_eq!(parse_table_u16(toml, "server", "port"), Some(3005));
    }

    #[test]
    fn parses_astro_dev_script_port() {
        assert_eq!(port_from_script("astro dev --port 4321"), Some(4321));
        assert_eq!(port_from_script("astro dev --port=4400"), Some(4400));
        assert_eq!(port_from_script("astro dev"), None);
        // The monitoring console's actual start script.
        assert_eq!(
            port_from_script("ng serve --port 4400 --proxy-config proxy.conf.json"),
            Some(4400)
        );
    }

    /// A service can listen on more than one port. One that came up without
    /// its second bound looks healthy and accepts nothing.
    #[test]
    fn a_services_extra_ports_are_checked_too() {
        let urls = DevUrls {
            api: None,
            app: None,
            www: None,
            admin: None,
            extra: vec![("tempo".into(), "http://localhost:3200".into())],
        };
        let extras = [super::super::selection::ExtraService {
            name: "tempo".into(),
            dir: "telemetry/tempo".into(),
            command: "tempo".into(),
            args: vec![],
            url: "http://localhost:3200".into(),
            requires: Some("tempo".into()),
            ports: vec![4318, 9095],
        }];
        let ports = ports_to_check(&urls, &extras);
        assert!(ports.contains(&3200), "the url port");
        assert!(ports.contains(&4318), "OTLP ingest");
        assert!(ports.contains(&9095), "gRPC");
    }

    #[test]
    fn extra_url_port_is_checked() {
        let sel = ServiceSelection {
            api: true,
            app: false,
            www: false,
        };
        let mut urls = discover_urls(Path::new("/nonexistent"), &sel);
        urls.extra.push((
            "vision".into(),
            "http://localhost:8765/tools/solve_studio/".into(),
        ));
        assert!(ports_to_check(&urls, &[]).contains(&8765));
    }
    #[test]
    fn extra_url_without_a_port_is_skipped() {
        let mut urls = DevUrls {
            api: None,
            app: None,
            www: None,
            admin: None,
            extra: vec![("local".into(), "http://localhost/tools/".into())],
        };
        assert!(ports_to_check(&urls, &[]).is_empty());
        urls.extra[0].1 = "http://localhost:8765/".into();
        assert_eq!(ports_to_check(&urls, &[]), vec![8765]);
    }
}
