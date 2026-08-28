//! Warmed CORS origin set for the collector.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Union of `project.cors_origins` and `[cors] allowed_origins`. Loaded at
//! boot, rewritten on project create, and reloaded on a short TTL so other
//! replicas pick up new origins. Distinct from the token-hash cache: a
//! preflight has no ingest key.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::http::{request::Parts, HeaderValue};
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait};
use tower_http::cors::{AllowOrigin, CorsLayer};

use super::models::project;

/// How long a warmed origin set is trusted before a reload.
pub const ORIGIN_SET_TTL: Duration = Duration::from_secs(60);

struct OriginSetInner {
    origins: HashSet<String>,
    loaded_at: Instant,
}

impl Default for OriginSetInner {
    fn default() -> Self {
        Self {
            origins: HashSet::new(),
            loaded_at: Instant::now(),
        }
    }
}

/// Exact Origin strings allowed by the collector CORS predicate.
#[derive(Clone, Default)]
pub struct OriginSet {
    inner: Arc<RwLock<OriginSetInner>>,
}

impl OriginSet {
    /// Empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this exact Origin string is allowed.
    #[must_use]
    pub fn contains(&self, origin: &str) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .origins
            .contains(origin)
    }

    /// Whether the warmed set is older than [`ORIGIN_SET_TTL`].
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .loaded_at
            .elapsed()
            >= ORIGIN_SET_TTL
    }

    /// Replace the whole set and reset the TTL clock.
    pub fn replace(&self, origins: HashSet<String>) {
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = OriginSetInner {
            origins,
            loaded_at: Instant::now(),
        };
    }

    /// Union origins into the set without resetting the TTL.
    ///
    /// Used after create so a failed full reload still allows the new project's
    /// origins on this replica.
    pub fn extend(&self, origins: impl IntoIterator<Item = String>) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .origins
            .extend(origins);
    }

    /// Current members, for tests.
    #[must_use]
    pub fn snapshot(&self) -> HashSet<String> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .origins
            .clone()
    }
}

/// Load every project's origins plus the configured extras.
pub async fn load_origins<C: ConnectionTrait>(
    db: &C,
    extras: &[String],
) -> Result<HashSet<String>, sea_orm::DbErr> {
    let mut origins: HashSet<String> = extras
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let projects = project::Entity::find().all(db).await?;
    for row in projects {
        origins.extend(origins_from_json(&row.cors_origins));
    }
    Ok(origins)
}

/// Refresh the warmed set from the database.
pub async fn refresh_origins(
    set: &OriginSet,
    db: &DatabaseConnection,
    extras: &[String],
) -> Result<(), sea_orm::DbErr> {
    set.replace(load_origins(db, extras).await?);
    Ok(())
}

/// Parse `cors_origins` JSON into origin strings.
#[must_use]
pub fn origins_from_json(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// One CORS layer whose predicate is the warmed origin set.
///
/// Applied only when the framework layer is skipped (`App.skip_default_cors`).
pub fn collector_cors_layer(origins: OriginSet) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(
            move |origin: &HeaderValue, _parts: &Parts| {
                origin
                    .to_str()
                    .map(|value| origins.contains(value))
                    .unwrap_or(false)
            },
        ))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
        .allow_credentials(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn union_is_exact_string_lookup() {
        let set = OriginSet::new();
        set.replace(HashSet::from([
            "https://app.teryon.com".to_string(),
            "http://localhost:4400".to_string(),
        ]));
        assert!(set.contains("https://app.teryon.com"));
        assert!(set.contains("http://localhost:4400"));
        assert!(!set.contains("https://app.cubeast.com"));
    }

    #[test]
    fn json_origins_skip_empties() {
        let value = json!(["https://a.test", "", "  ", "https://b.test"]);
        assert_eq!(
            origins_from_json(&value),
            vec!["https://a.test".to_string(), "https://b.test".to_string()]
        );
    }

    #[test]
    fn extend_unions_without_dropping_existing_members() {
        let set = OriginSet::new();
        set.replace(HashSet::from(["https://a.test".to_string()]));
        set.extend(["https://b.test".to_string()]);
        assert!(set.contains("https://a.test"));
        assert!(set.contains("https://b.test"));
        assert!(!set.is_stale());
    }
}
