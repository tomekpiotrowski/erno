//! Ingest credentials and caller identification.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Two tokens, deliberately not one:
//!
//! * **server** — a real secret, held only by Erno API processes. The trusted
//!   path: user attribution on the wire is believed.
//! * **browser** — ships inside the JS bundle and is therefore **public**. A
//!   speed bump against drive-by scanners, not a security control. Untrusted:
//!   attribution is discarded, because otherwise anyone could pin errors on any
//!   user.
//!
//! Keeping them separate means the public one can be rotated without
//! redeploying the API, and a leaked public one can never impersonate a server.

use std::net::IpAddr;

use axum::http::HeaderMap;

use crate::error_reporting::{config::CollectorConfig, Source};

use super::dto::IngestOrigin;

/// Header carrying the ingest token.
pub const INGEST_KEY_HEADER: &str = "x-erno-ingest-key";
/// Header a browser uses to label itself `app` or `admin`.
pub const SOURCE_HEADER: &str = "x-erno-source";

/// Which rate-limit tier a caller falls into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestIdentity {
    /// Assigned source and trust level.
    pub origin: IngestOrigin,
    /// Rate-limit bucket key.
    pub rate_limit_key: String,
    /// Rate-limit action name.
    pub rate_limit_action: &'static str,
}

/// Identify an ingest caller from its token.
///
/// Returns `None` when the token is absent or matches neither configured value.
#[must_use]
pub fn authenticate(
    config: &CollectorConfig,
    headers: &HeaderMap,
    client_ip: Option<IpAddr>,
) -> Option<IngestIdentity> {
    let presented = headers.get(INGEST_KEY_HEADER)?.to_str().ok()?.trim();

    if !config.server_token.is_empty() && constant_time_eq(presented, &config.server_token) {
        return Some(IngestIdentity {
            origin: IngestOrigin {
                source: declared_source(headers).unwrap_or(Source::Api),
                trusted: true,
            },
            // One trusted sender per deployment, so a single bucket is right.
            rate_limit_key: "token:server".to_string(),
            rate_limit_action: "error_ingest_server",
        });
    }

    if !config.browser_token.is_empty() && constant_time_eq(presented, &config.browser_token) {
        // A browser may label itself, but only as one of the browser sources —
        // it can never claim to be the API.
        let source = match declared_source(headers) {
            Some(Source::Admin) => Source::Admin,
            _ => Source::App,
        };
        return Some(IngestIdentity {
            origin: IngestOrigin {
                source,
                trusted: false,
            },
            rate_limit_key: client_ip
                .map_or_else(|| "ip:unknown".to_string(), |ip| format!("ip:{ip}")),
            rate_limit_action: "error_ingest_browser",
        });
    }

    None
}

/// Whether `Authorization: Bearer` matches the trusted **server** ingest token.
///
/// Used by nginx `auth_request` in front of Tempo/Loki OTLP ingest. The public
/// browser token is never accepted: traces and logs are server-side only.
#[must_use]
pub fn authenticate_server_bearer(config: &CollectorConfig, headers: &HeaderMap) -> bool {
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    !config.server_token.is_empty() && constant_time_eq(presented, &config.server_token)
}

fn declared_source(headers: &HeaderMap) -> Option<Source> {
    Source::from_str_opt(headers.get(SOURCE_HEADER)?.to_str().ok()?.trim())
}

/// Compare two secrets without leaking their common prefix length through
/// timing. Length difference is not hidden, which is fine for a token whose
/// length is fixed by configuration.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        difference |= x ^ y;
    }
    difference == 0
}

/// Resolve the reporting client's address.
///
/// Proxy headers are honoured only when the deployment says it is behind a
/// trusted proxy; otherwise anyone could spoof their way into a fresh
/// rate-limit bucket.
#[must_use]
pub fn resolve_client_ip(
    headers: &HeaderMap,
    socket_ip: Option<IpAddr>,
    trust_proxy: bool,
) -> Option<IpAddr> {
    if trust_proxy {
        if let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
        {
            return Some(ip);
        }
        if let Some(ip) = headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
        {
            return Some(ip);
        }
    }
    socket_ip
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> CollectorConfig {
        CollectorConfig {
            server_token: "server-secret".to_string(),
            browser_token: "browser-public".to_string(),
            ..CollectorConfig::default()
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        map
    }

    #[test]
    fn no_token_is_rejected() {
        assert!(authenticate(&config(), &headers(&[]), None).is_none());
    }

    #[test]
    fn a_wrong_token_is_rejected() {
        let h = headers(&[(INGEST_KEY_HEADER, "nope")]);
        assert!(authenticate(&config(), &h, None).is_none());
    }

    #[test]
    fn the_server_token_is_trusted_and_defaults_to_the_api_source() {
        let h = headers(&[(INGEST_KEY_HEADER, "server-secret")]);
        let identity = authenticate(&config(), &h, None).expect("authenticated");
        assert!(identity.origin.trusted);
        assert_eq!(identity.origin.source, Source::Api);
        assert_eq!(identity.rate_limit_action, "error_ingest_server");
    }

    #[test]
    fn the_browser_token_is_untrusted() {
        let h = headers(&[(INGEST_KEY_HEADER, "browser-public")]);
        let identity = authenticate(&config(), &h, None).expect("authenticated");
        assert!(!identity.origin.trusted);
        assert_eq!(identity.origin.source, Source::App);
        assert_eq!(identity.rate_limit_action, "error_ingest_browser");
    }

    #[test]
    fn a_browser_may_label_itself_admin_but_never_api() {
        let h = headers(&[
            (INGEST_KEY_HEADER, "browser-public"),
            (SOURCE_HEADER, "admin"),
        ]);
        assert_eq!(
            authenticate(&config(), &h, None).unwrap().origin.source,
            Source::Admin
        );

        // The security-relevant case: claiming to be the trusted server source.
        let h = headers(&[
            (INGEST_KEY_HEADER, "browser-public"),
            (SOURCE_HEADER, "api"),
        ]);
        let identity = authenticate(&config(), &h, None).unwrap();
        assert_eq!(identity.origin.source, Source::App);
        assert!(!identity.origin.trusted);
    }

    #[test]
    fn browsers_are_bucketed_per_ip() {
        let h = headers(&[(INGEST_KEY_HEADER, "browser-public")]);
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        let identity = authenticate(&config(), &h, Some(ip)).unwrap();
        assert_eq!(identity.rate_limit_key, "ip:203.0.113.7");
    }

    #[test]
    fn an_unset_token_never_authenticates_even_against_an_empty_header() {
        let config = CollectorConfig {
            server_token: String::new(),
            browser_token: String::new(),
            ..CollectorConfig::default()
        };
        let h = headers(&[(INGEST_KEY_HEADER, "")]);
        assert!(authenticate(&config, &h, None).is_none());
    }

    #[test]
    fn constant_time_eq_matches_plain_equality() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn proxy_headers_are_ignored_unless_trusted() {
        let h = headers(&[("x-forwarded-for", "198.51.100.1, 10.0.0.1")]);
        let socket: IpAddr = "10.0.0.5".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&h, Some(socket), false),
            Some(socket),
            "spoofable headers must not win when the proxy is untrusted"
        );
        assert_eq!(
            resolve_client_ip(&h, Some(socket), true),
            Some("198.51.100.1".parse::<IpAddr>().unwrap()),
            "leftmost X-Forwarded-For is the real client"
        );
    }

    #[test]
    fn server_bearer_is_accepted_and_the_browser_token_is_not() {
        let ok = headers(&[("authorization", "Bearer server-secret")]);
        assert!(authenticate_server_bearer(&config(), &ok));

        let browser = headers(&[("authorization", "Bearer browser-public")]);
        assert!(!authenticate_server_bearer(&config(), &browser));

        let missing = headers(&[]);
        assert!(!authenticate_server_bearer(&config(), &missing));
    }

    #[test]
    fn x_real_ip_is_the_fallback_when_trusted() {
        let h = headers(&[("x-real-ip", "198.51.100.9")]);
        assert_eq!(
            resolve_client_ip(&h, None, true),
            Some("198.51.100.9".parse::<IpAddr>().unwrap())
        );
    }
}
