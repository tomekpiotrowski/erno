//! Ingest credentials and caller identification.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Two tokens per project, deliberately not one:
//!
//! * **server** — a real secret, held only by Erno API processes. The trusted
//!   path: user attribution on the wire is believed.
//! * **browser** — ships inside the JS bundle and is therefore **public**. A
//!   speed bump against drive-by scanners, not a security control. Untrusted:
//!   attribution is discarded, because otherwise anyone could pin errors on any
//!   user.
//!
//! Tokens are stored as SHA-256 hex on the project row. Lookup is by hash, two
//! queries, never a blind `OR`. Empty stored hashes never match.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use erno::error_reporting::Source;
use erno::token::hash_token;

use super::dto::IngestOrigin;
use super::models::project;

/// Header carrying the ingest token.
pub const INGEST_KEY_HEADER: &str = "x-erno-ingest-key";
/// Header a browser uses to label itself `app` or `admin`.
pub const SOURCE_HEADER: &str = "x-erno-source";

const TOKEN_CACHE_TTL: Duration = Duration::from_secs(60);

/// Which credential matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Trusted server-to-server path.
    Server,
    /// Public browser path.
    Browser,
}

/// Which rate-limit tier a caller falls into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestIdentity {
    /// Assigned source and trust level.
    pub origin: IngestOrigin,
    /// Project the token belongs to.
    pub project_id: Uuid,
    /// Project slug, for metrics labels.
    pub project_slug: String,
    /// Rate-limit bucket key.
    pub rate_limit_key: String,
    /// Rate-limit action name.
    pub rate_limit_action: &'static str,
}

#[derive(Debug, Clone)]
struct CachedToken {
    kind: TokenKind,
    project_id: Uuid,
    project_slug: String,
    expires_at: Instant,
}

/// In-memory hash → identity cache. Misses load from the database; rotate and
/// delete invalidate. Never stores plaintext.
#[derive(Clone, Default)]
pub struct TokenCache {
    inner: Arc<Mutex<HashMap<String, CachedToken>>>,
}

impl TokenCache {
    /// Empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, hash: &str) -> Option<(TokenKind, Uuid, String)> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.get(hash)?;
        if entry.expires_at <= Instant::now() {
            map.remove(hash);
            return None;
        }
        Some((entry.kind, entry.project_id, entry.project_slug.clone()))
    }

    fn insert(&self, hash: String, kind: TokenKind, project_id: Uuid, project_slug: String) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(
            hash,
            CachedToken {
                kind,
                project_id,
                project_slug,
                expires_at: Instant::now() + TOKEN_CACHE_TTL,
            },
        );
    }

    /// Drop a stored hash so the next lookup hits the database.
    pub fn invalidate(&self, hash: &str) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(hash);
    }
}

/// Identify an ingest caller from its token.
///
/// Returns `None` when the token is absent, empty, or matches no project.
pub async fn authenticate<C: ConnectionTrait>(
    db: &C,
    cache: &TokenCache,
    headers: &HeaderMap,
    client_ip: Option<IpAddr>,
) -> Option<IngestIdentity> {
    let presented = presented_ingest_key(headers)?;
    let (kind, project_id, project_slug) = lookup_token(db, cache, presented).await?;
    Some(identity_from_match(
        kind,
        project_id,
        project_slug,
        headers,
        client_ip,
    ))
}

/// Whether `Authorization: Bearer` matches a project's trusted **server**
/// ingest token.
///
/// Used by nginx `auth_request` in front of Tempo/Loki OTLP ingest. The public
/// browser token is never accepted: traces and logs are server-side only.
pub async fn authenticate_server_bearer<C: ConnectionTrait>(
    db: &C,
    cache: &TokenCache,
    headers: &HeaderMap,
) -> Option<(Uuid, String)> {
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    match lookup_token(db, cache, presented).await {
        Some((TokenKind::Server, project_id, slug)) => Some((project_id, slug)),
        _ => None,
    }
}

async fn lookup_token<C: ConnectionTrait>(
    db: &C,
    cache: &TokenCache,
    presented: &str,
) -> Option<(TokenKind, Uuid, String)> {
    let hash = hash_token(presented);
    if let Some(hit) = cache.get(&hash) {
        metrics::counter!("erno_project_token_lookup_total", "result" => "hit").increment(1);
        return Some(hit);
    }

    if let Some(row) = find_by_server_hash(db, &hash).await {
        cache.insert(hash, TokenKind::Server, row.id, row.slug.clone());
        metrics::counter!("erno_project_token_lookup_total", "result" => "miss").increment(1);
        return Some((TokenKind::Server, row.id, row.slug));
    }

    if let Some(row) = find_by_browser_hash(db, &hash).await {
        cache.insert(hash, TokenKind::Browser, row.id, row.slug.clone());
        metrics::counter!("erno_project_token_lookup_total", "result" => "miss").increment(1);
        return Some((TokenKind::Browser, row.id, row.slug));
    }

    metrics::counter!("erno_project_token_lookup_total", "result" => "unknown").increment(1);
    None
}

async fn find_by_server_hash<C: ConnectionTrait>(db: &C, hash: &str) -> Option<project::Model> {
    project::Entity::find()
        .filter(project::Column::ServerTokenHash.eq(hash))
        .filter(project::Column::ServerTokenHash.ne(""))
        .one(db)
        .await
        .ok()
        .flatten()
}

async fn find_by_browser_hash<C: ConnectionTrait>(db: &C, hash: &str) -> Option<project::Model> {
    project::Entity::find()
        .filter(project::Column::BrowserTokenHash.eq(hash))
        .filter(project::Column::BrowserTokenHash.ne(""))
        .one(db)
        .await
        .ok()
        .flatten()
}

fn identity_from_match(
    kind: TokenKind,
    project_id: Uuid,
    project_slug: String,
    headers: &HeaderMap,
    client_ip: Option<IpAddr>,
) -> IngestIdentity {
    match kind {
        TokenKind::Server => IngestIdentity {
            origin: IngestOrigin {
                source: declared_source(headers).unwrap_or(Source::Api),
                trusted: true,
            },
            project_id,
            project_slug,
            rate_limit_key: format!("token:server:{project_id}"),
            rate_limit_action: "error_ingest_server",
        },
        TokenKind::Browser => {
            let source = match declared_source(headers) {
                Some(Source::Admin) => Source::Admin,
                _ => Source::App,
            };
            IngestIdentity {
                origin: IngestOrigin {
                    source,
                    trusted: false,
                },
                project_id,
                project_slug,
                rate_limit_key: client_ip.map_or_else(
                    || format!("ip:unknown:{project_id}"),
                    |ip| format!("ip:{ip}:{project_id}"),
                ),
                rate_limit_action: "error_ingest_browser",
            }
        }
    }
}

fn declared_source(headers: &HeaderMap) -> Option<Source> {
    Source::from_str_opt(headers.get(SOURCE_HEADER)?.to_str().ok()?.trim())
}

/// Presented ingest key, or `None` when the header is missing or empty.
fn presented_ingest_key(headers: &HeaderMap) -> Option<&str> {
    let presented = headers.get(INGEST_KEY_HEADER)?.to_str().ok()?.trim();
    if presented.is_empty() {
        None
    } else {
        Some(presented)
    }
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

    fn project_id() -> Uuid {
        Uuid::from_u128(1)
    }

    #[test]
    fn the_server_token_is_trusted_and_defaults_to_the_api_source() {
        let identity = identity_from_match(
            TokenKind::Server,
            project_id(),
            "teryon".to_string(),
            &headers(&[]),
            None,
        );
        assert!(identity.origin.trusted);
        assert_eq!(identity.origin.source, Source::Api);
        assert_eq!(identity.rate_limit_action, "error_ingest_server");
        assert_eq!(
            identity.rate_limit_key,
            format!("token:server:{}", project_id())
        );
        assert_eq!(identity.project_slug, "teryon");
    }

    #[test]
    fn the_browser_token_is_untrusted() {
        let identity = identity_from_match(
            TokenKind::Browser,
            project_id(),
            "teryon".to_string(),
            &headers(&[]),
            None,
        );
        assert!(!identity.origin.trusted);
        assert_eq!(identity.origin.source, Source::App);
        assert_eq!(identity.rate_limit_action, "error_ingest_browser");
    }

    #[test]
    fn a_browser_may_label_itself_admin_but_never_api() {
        let h = headers(&[(SOURCE_HEADER, "admin")]);
        assert_eq!(
            identity_from_match(TokenKind::Browser, project_id(), "t".into(), &h, None)
                .origin
                .source,
            Source::Admin
        );

        let h = headers(&[(SOURCE_HEADER, "api")]);
        let identity = identity_from_match(TokenKind::Browser, project_id(), "t".into(), &h, None);
        assert_eq!(identity.origin.source, Source::App);
        assert!(!identity.origin.trusted);
    }

    #[test]
    fn browsers_are_bucketed_per_ip_and_project() {
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        let identity = identity_from_match(
            TokenKind::Browser,
            project_id(),
            "t".into(),
            &headers(&[]),
            Some(ip),
        );
        assert_eq!(
            identity.rate_limit_key,
            format!("ip:203.0.113.7:{}", project_id())
        );
    }

    #[test]
    fn an_empty_presented_token_is_rejected_before_lookup() {
        assert!(presented_ingest_key(&headers(&[])).is_none());
        assert!(presented_ingest_key(&headers(&[(INGEST_KEY_HEADER, "")])).is_none());
        assert!(presented_ingest_key(&headers(&[(INGEST_KEY_HEADER, "   ")])).is_none());
        assert_eq!(
            presented_ingest_key(&headers(&[(INGEST_KEY_HEADER, "erns_x")])),
            Some("erns_x")
        );
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
    fn x_real_ip_is_the_fallback_when_trusted() {
        let h = headers(&[("x-real-ip", "198.51.100.9")]);
        assert_eq!(
            resolve_client_ip(&h, None, true),
            Some("198.51.100.9".parse::<IpAddr>().unwrap())
        );
    }

    #[test]
    fn token_cache_expires_and_invalidates() {
        let cache = TokenCache::new();
        let hash = "abc".to_string();
        cache.insert(hash.clone(), TokenKind::Server, project_id(), "t".into());
        assert!(cache.get(&hash).is_some());
        cache.invalidate(&hash);
        assert!(cache.get(&hash).is_none());
    }
}
