//! Secret redaction for error payloads.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! The same rule set is implemented twice — here and in
//! `app/projects/erno-angular/src/lib/errors/scrub.ts` — and both are
//! necessary. The client-side pass is the only thing that can stop a secret
//! *crossing the network* to the collector's provider; this one is the only
//! thing that can stop it being *stored*, and it is the only pass at all for
//! `source = api` events, which never had a client.
//!
//! Redaction is visible: values become the literal `[redacted]` rather than
//! disappearing, because during triage a mystery absence is worse than a
//! marked one.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

/// What a redacted value is replaced with.
pub const REDACTED: &str = "[redacted]";
/// What an over-deep structure is replaced with.
pub const TRUNCATED: &str = "[truncated: too deep]";
/// How far into nested objects and arrays scrubbing will descend.
const MAX_DEPTH: usize = 5;

/// Key name fragments that mark a value as sensitive.
///
/// Matched against *tokens* of the key rather than as raw substrings, so
/// `author` is not mistaken for `auth` and `standard` is not mistaken for
/// `card`. See [`is_sensitive_key`].
const SENSITIVE_TOKENS: [&str; 22] = [
    "auth",
    "authorization",
    "token",
    "jwt",
    "password",
    "passwd",
    "pwd",
    "secret",
    "apikey",
    "key",
    "credential",
    "credentials",
    "cookie",
    "cookies",
    "session",
    "csrf",
    "xsrf",
    "signature",
    "card",
    "cvv",
    "cvc",
    "ssn",
];

/// Split an arbitrary key into lowercase word tokens, breaking on
/// non-alphanumeric characters and camelCase boundaries.
fn key_tokens(key: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = key.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        // camelCase / PascalCase boundary: a capital preceded by a lowercase
        // or digit starts a new token.
        if ch.is_uppercase()
            && i > 0
            && (chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit())
            && !current.is_empty()
        {
            tokens.push(std::mem::take(&mut current));
        }
        current.push(ch.to_ascii_lowercase());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Whether a key name marks its value as sensitive.
#[must_use]
pub fn is_sensitive_key(key: &str) -> bool {
    let tokens = key_tokens(key);
    if tokens
        .iter()
        .any(|t| SENSITIVE_TOKENS.contains(&t.as_str()))
    {
        return true;
    }
    // Also catch unsplittable compounds like `access_token` written as
    // `accesstoken`, where tokenisation yields one long word.
    let joined = tokens.concat();
    ["token", "password", "secret", "apikey", "cookie"]
        .iter()
        .any(|needle| joined.contains(needle))
}

/// Redact secret-shaped substrings inside a free-text value.
#[must_use]
pub fn scrub_text(input: &str) -> String {
    static JWT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"eyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]*").expect("valid regex")
    });
    static SCHEME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{6,}").expect("valid regex")
    });
    static CARD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b(?:\d[ -]?){13,19}\b").expect("valid regex"));
    static OPAQUE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9_-]{32,}\b").expect("valid regex"));

    static UUID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
            .expect("valid regex")
    });

    // JWTs first — they contain dots and would otherwise be partly eaten by the
    // opaque-run rule, leaving a recognisable header behind.
    let out = JWT.replace_all(input, REDACTED);
    let out = SCHEME.replace_all(&out, REDACTED);
    let out = CARD.replace_all(&out, REDACTED);
    // A UUID is long enough to look opaque but is an identifier, not a
    // credential. Redacting it would throw away the one detail that tells an
    // operator *which* record or user an error was about.
    let out = OPAQUE.replace_all(&out, |caps: &regex::Captures<'_>| {
        let matched = &caps[0];
        if UUID.is_match(matched) {
            matched.to_string()
        } else {
            REDACTED.to_string()
        }
    });
    out.into_owned()
}

/// Scrub a URL: the fragment is dropped wholesale and sensitive query
/// parameters are redacted.
///
/// Dropping the fragment is not paranoia — Erno's own share links carry their
/// secret there (see `api/src/share/extractor.rs`), so a shared-link URL in an
/// error report would otherwise leak access to the shared resource.
#[must_use]
pub fn scrub_url(url: &str) -> String {
    let without_fragment = url.split_once('#').map_or(url, |(head, _)| head);

    let Some((path, query)) = without_fragment.split_once('?') else {
        return scrub_text(without_fragment);
    };

    let scrubbed: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if is_sensitive_key(k) => format!("{k}={REDACTED}"),
            Some((k, v)) => format!("{k}={}", scrub_text(v)),
            None => scrub_text(pair),
        })
        .collect();

    format!("{}?{}", scrub_text(path), scrubbed.join("&"))
}

/// Recursively scrub a JSON payload in place.
///
/// Object values under a sensitive key are replaced wholesale; every other
/// string is passed through [`scrub_text`]. Recursion stops at `MAX_DEPTH` (5)
/// so a hostile or accidentally cyclic-looking payload cannot blow the stack.
pub fn scrub_value(value: &mut Value) {
    scrub_value_at(value, 0);
}

fn scrub_value_at(value: &mut Value, depth: usize) {
    match value {
        Value::String(s) => *s = scrub_text(s),
        Value::Array(items) => {
            if depth >= MAX_DEPTH {
                *value = Value::String(TRUNCATED.to_string());
                return;
            }
            for item in items.iter_mut() {
                scrub_value_at(item, depth + 1);
            }
        }
        Value::Object(map) => {
            if depth >= MAX_DEPTH {
                *value = Value::String(TRUNCATED.to_string());
                return;
            }
            scrub_object_at(map, depth);
        }
        _ => {}
    }
}

fn scrub_object_at(map: &mut Map<String, Value>, depth: usize) {
    for (key, child) in map.iter_mut() {
        if is_sensitive_key(key) {
            *child = Value::String(REDACTED.to_string());
            continue;
        }
        // `url` fields get the query/fragment treatment rather than plain text.
        if key.eq_ignore_ascii_case("url") || key.eq_ignore_ascii_case("referrer") {
            if let Value::String(s) = child {
                *s = scrub_url(s);
                continue;
            }
        }
        scrub_value_at(child, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sensitive_keys_are_recognised() {
        for key in [
            "authorization",
            "Authorization",
            "auth",
            "access_token",
            "accessToken",
            "refresh_token",
            "id_token",
            "password",
            "userPassword",
            "api_key",
            "apiKey",
            "Cookie",
            "set-cookie",
            "session",
            "csrf_token",
            "private_key",
            "signature",
            "creditCard",
            "cvv",
            "ssn",
        ] {
            assert!(is_sensitive_key(key), "expected {key} to be sensitive");
        }
    }

    #[test]
    fn innocent_keys_that_merely_contain_a_needle_are_kept() {
        // The reason keys are tokenised instead of substring-matched.
        for key in [
            "author",
            "authored_at",
            "standard",
            "discard",
            "cardinality",
            "sessionless_mode_description",
        ] {
            assert!(!is_sensitive_key(key), "expected {key} to be kept");
        }
    }

    #[test]
    fn jwts_are_redacted() {
        let text = "failed with eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdEFGH1234 here";
        let out = scrub_text(text);
        assert!(!out.contains("eyJ"), "got {out}");
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn bearer_and_basic_schemes_are_redacted() {
        assert!(!scrub_text("Authorization: Bearer abc123def456").contains("abc123def456"));
        assert!(!scrub_text("Basic dXNlcjpwYXNzd29yZA==").contains("dXNlcjpwYXNzd29yZA"));
    }

    #[test]
    fn uuids_survive_redaction() {
        // They are identifiers, not secrets — and they are exactly what an
        // operator needs to trace an error back to a record.
        let id = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            scrub_text(&format!("sync failed for user {id}")),
            format!("sync failed for user {id}")
        );
        // A same-length non-UUID run is still treated as opaque.
        let opaque = "550e8400e29b41d4a716446655440000";
        assert_eq!(scrub_text(opaque), REDACTED);
    }

    #[test]
    fn long_opaque_runs_are_redacted() {
        let secret = "a".repeat(40);
        assert_eq!(scrub_text(&secret), REDACTED);
        // Short values are left alone.
        assert_eq!(scrub_text("shortvalue"), "shortvalue");
    }

    #[test]
    fn card_numbers_are_redacted() {
        assert!(!scrub_text("card 4111 1111 1111 1111 declined").contains("4111"));
    }

    #[test]
    fn url_fragments_are_dropped_entirely() {
        // Erno share links carry their secret in the fragment.
        let out = scrub_url("https://app.test/shared#tok=supersecretvalue");
        assert!(!out.contains("supersecretvalue"), "got {out}");
        assert!(!out.contains('#'));
    }

    #[test]
    fn sensitive_query_params_are_redacted_and_others_kept() {
        let out = scrub_url("https://app.test/x?page=2&access_token=abc123&sort=name");
        assert!(out.contains("page=2"), "got {out}");
        assert!(out.contains("sort=name"), "got {out}");
        assert!(
            out.contains(&format!("access_token={REDACTED}")),
            "got {out}"
        );
        assert!(!out.contains("abc123"));
    }

    #[test]
    fn url_without_query_is_untouched() {
        assert_eq!(
            scrub_url("https://app.test/decks/42"),
            "https://app.test/decks/42"
        );
    }

    #[test]
    fn nested_objects_are_scrubbed() {
        let mut value = json!({
            "route": "/decks/:id",
            "headers": { "authorization": "Bearer abc123def456", "accept": "application/json" },
            "extra": { "nested": { "password": "hunter2", "keep": "fine" } },
            "url": "https://app.test/x?token=abc#frag"
        });
        scrub_value(&mut value);

        assert_eq!(value["headers"]["authorization"], REDACTED);
        assert_eq!(value["headers"]["accept"], "application/json");
        assert_eq!(value["extra"]["nested"]["password"], REDACTED);
        assert_eq!(value["extra"]["nested"]["keep"], "fine");
        assert_eq!(value["route"], "/decks/:id");
        let url = value["url"].as_str().unwrap();
        assert!(!url.contains("frag"), "got {url}");
        assert!(url.contains(&format!("token={REDACTED}")), "got {url}");
    }

    #[test]
    fn arrays_are_scrubbed() {
        let mut value = json!({ "items": ["fine", "Bearer abcdef123456"] });
        scrub_value(&mut value);
        assert_eq!(value["items"][0], "fine");
        assert_eq!(value["items"][1], REDACTED);
    }

    #[test]
    fn recursion_stops_at_max_depth() {
        // Build a structure deeper than MAX_DEPTH.
        let mut value = json!({ "password": "leaf" });
        for _ in 0..10 {
            value = json!({ "next": value });
        }
        scrub_value(&mut value);
        let rendered = value.to_string();
        // Either it was scrubbed or the branch was truncated; the secret must
        // not survive either way.
        assert!(!rendered.contains("leaf"), "got {rendered}");
        assert!(rendered.contains(TRUNCATED), "got {rendered}");
    }

    #[test]
    fn non_string_scalars_are_left_alone() {
        let mut value = json!({ "count": 3, "ok": true, "nothing": null });
        scrub_value(&mut value);
        assert_eq!(value["count"], 3);
        assert_eq!(value["ok"], true);
        assert!(value["nothing"].is_null());
    }
}
