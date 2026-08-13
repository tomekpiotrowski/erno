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
    }
}

pub fn ports_to_check(urls: &DevUrls) -> Vec<u16> {
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
    ports
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
    let content = std::fs::read_to_string(root.join("www/package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let script = json["scripts"]["dev"].as_str()?;
    port_from_script(script)
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
    }
}
