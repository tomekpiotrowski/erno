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
    let module_path = app_dir.join("src/app/app.module.ts");
    let path = if env_path.is_file() {
        env_path
    } else if module_path.is_file() {
        module_path
    } else {
        return Err(format!(
            "cannot find src/environments/environment.ts or src/app/app.module.ts to point the app at {api_http}"
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
    fn cors_list_includes_capacitor() {
        let list = cors_origins("http://192.168.1.5:4200");
        assert!(list.contains("capacitor://localhost"));
        assert!(list.contains("http://192.168.1.5:4200"));
    }
}
