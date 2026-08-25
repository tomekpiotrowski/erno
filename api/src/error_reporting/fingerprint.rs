//! Grouping-key computation.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Pure by design: no database, no configuration, no I/O. This runs **only at
//! the collector** — clients are never trusted to group, because a client bug
//! is exactly what produces bad grouping.
//!
//! The single most important property, and the one the tests pin down: **line
//! and column numbers are excluded from the hash.** They are kept on the event
//! for display, but including them would mint a brand-new issue on every deploy
//! that touched the file, which is the usual way homegrown groupers fail.

use std::sync::LazyLock;

use regex::Regex;

use super::{Frame, Source};

/// How many stack frames participate in the grouping key.
const FRAME_DEPTH: usize = 5;
/// Maximum client-supplied fingerprint parts honoured.
const MAX_CLIENT_PARTS: usize = 8;
/// Maximum length of a single client-supplied fingerprint part.
const MAX_CLIENT_PART_LEN: usize = 64;
/// Maximum length of a normalized message used as a grouping key.
const MAX_MESSAGE_KEY_LEN: usize = 200;

/// Everything the grouping key is derived from.
#[derive(Debug, Clone)]
pub struct FingerprintInput<'a> {
    /// Which component reported. Always the first part, so a browser
    /// `TypeError` and a Rust panic can never collide.
    pub source: Source,
    /// Exception type, error class, or tracing target.
    pub error_type: &'a str,
    /// The error message, used only when there are no frames.
    pub message: &'a str,
    /// Stack frames, nearest-to-throw first.
    pub frames: &'a [Frame],
    /// Explicit client override. Honoured verbatim, still namespaced by source.
    pub client_fingerprint: Option<&'a [String]>,
    /// For stackless captures (a `tracing::error!`), the `file` of the call
    /// site. A log statement's location is a better key than its message.
    pub call_site: Option<&'a str>,
}

/// Compute the 64-character hex grouping key.
#[must_use]
pub fn fingerprint(input: &FingerprintInput<'_>) -> String {
    crate::token::hash_token(&fingerprint_parts(input).join("\n"))
}

/// The parts that feed the hash. Exposed so tests can assert on the *reason*
/// two errors grouped, not just that their digests matched.
#[must_use]
pub fn fingerprint_parts(input: &FingerprintInput<'_>) -> Vec<String> {
    let mut parts = vec![input.source.as_str().to_string()];

    // An explicit client fingerprint wins, but stays namespaced by source.
    if let Some(client) = input.client_fingerprint {
        let supplied: Vec<String> = client
            .iter()
            .filter(|p| !p.trim().is_empty())
            .take(MAX_CLIENT_PARTS)
            .map(|p| truncate_chars(p.trim(), MAX_CLIENT_PART_LEN))
            .collect();
        if !supplied.is_empty() {
            parts.extend(supplied);
            return parts;
        }
    }

    parts.push(normalize_type(input.error_type));

    let selected = select_frames(input.frames);
    if selected.is_empty() {
        // No stack: fall back to the call site plus a normalized message.
        if let Some(site) = input.call_site {
            parts.push(normalize_file(site));
        }
        parts.push(normalize_message(input.message));
    } else {
        for frame in selected {
            parts.push(format!(
                "{}@{}",
                frame
                    .function
                    .as_deref()
                    .map_or_else(|| "<anonymous>".to_string(), normalize_function),
                frame
                    .file
                    .as_deref()
                    .map_or_else(|| "<unknown>".to_string(), normalize_file),
            ));
        }
    }

    parts
}

/// Pick the frames that participate in grouping: in-app frames when any exist,
/// otherwise everything, capped at `FRAME_DEPTH` (5).
#[must_use]
pub fn select_frames(frames: &[Frame]) -> Vec<&Frame> {
    let in_app: Vec<&Frame> = frames
        .iter()
        .filter(|f| is_in_app(f.file.as_deref()))
        .collect();

    let pool = if in_app.is_empty() {
        frames.iter().collect::<Vec<_>>()
    } else {
        in_app
    };

    pool.into_iter().take(FRAME_DEPTH).collect()
}

/// Whether a file path looks like first-party code rather than a dependency or
/// runtime. Used both for grouping and to dim vendor frames in the UI.
#[must_use]
pub fn is_in_app(file: Option<&str>) -> bool {
    let Some(file) = file else {
        return false;
    };
    const VENDOR_MARKERS: [&str; 9] = [
        "node_modules/",
        "/.cargo/registry",
        "/rustc/",
        "zone.js",
        "/rxjs/",
        "core.mjs",
        "/@angular/",
        "polyfills",
        "/std/src/",
    ];
    !VENDOR_MARKERS.iter().any(|m| file.contains(m))
}

/// Normalize an exception type or tracing target.
#[must_use]
pub fn normalize_type(error_type: &str) -> String {
    let stripped = strip_generics(error_type.trim());
    collapse_whitespace(&stripped)
}

/// Normalize a function name so cosmetic codegen differences do not split an
/// issue: generic arguments, closure suffixes, Rust symbol hashes, and JS
/// call-site prefixes all come off.
#[must_use]
pub fn normalize_function(function: &str) -> String {
    static RUST_HASH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"::h[0-9a-f]{16}$").expect("valid regex"));
    static CLOSURE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"::\{\{closure\}\}|::\{closure#\d+\}|::\{\{constructor\}\}")
            .expect("valid regex")
    });

    let mut name = function.trim().to_string();

    // JS call-site decorations that vary with how the frame was reached.
    for prefix in ["async ", "new ", "bound ", "Object.", "Module.", "at "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            name = rest.to_string();
        }
    }

    name = RUST_HASH.replace(&name, "").into_owned();
    name = CLOSURE.replace_all(&name, "").into_owned();
    name = strip_generics(&name);

    collapse_whitespace(&name)
}

/// Normalize a file path or bundle URL so the same source file groups
/// consistently across hosts, deploys, and build hashes.
#[must_use]
pub fn normalize_file(file: &str) -> String {
    let mut path = file.trim().to_string();

    // Bundler pseudo-schemes first — they wrap a real path.
    for prefix in [
        "webpack-internal:///",
        "webpack://",
        "vite:",
        "file://",
        "rsc://",
    ] {
        if let Some(rest) = path.strip_prefix(prefix) {
            path = rest.to_string();
        }
    }

    // Strip scheme + host from real URLs, keeping the path.
    if let Some(idx) = path.find("://") {
        let after_scheme = &path[idx + 3..];
        path = after_scheme
            .find('/')
            .map_or_else(String::new, |slash| after_scheme[slash..].to_string());
    }

    // Query strings and fragments never identify the file.
    if let Some(idx) = path.find('?') {
        path.truncate(idx);
    }
    if let Some(idx) = path.find('#') {
        path.truncate(idx);
    }

    path = path.replace('\\', "/");
    path = rewrite_cargo_registry(&path);
    path = collapse_path(&path);
    strip_content_hash(&path)
}

/// `/home/x/.cargo/registry/src/index.crates.io-6f17d22bba15001f/sqlx-core-0.8.2/src/pool.rs`
/// becomes `sqlx-core/src/pool.rs`, so a dependency bump does not resplit issues.
fn rewrite_cargo_registry(path: &str) -> String {
    static REGISTRY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r".*/\.cargo/registry/src/[^/]+/([A-Za-z0-9_.-]+?)-\d+(?:\.\d+)*(?:[-+][0-9A-Za-z.-]+)?/(.*)$")
            .expect("valid regex")
    });

    REGISTRY.captures(path).map_or_else(
        || path.to_string(),
        |caps| format!("{}/{}", &caps[1], &caps[2]),
    )
}

/// Keep the meaningful tail of a path: everything from the last `src/` segment
/// onward, or failing that the last three segments.
fn collapse_path(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return String::new();
    }

    if let Some(idx) = segments.iter().rposition(|s| *s == "src") {
        return segments[idx..].join("/");
    }

    let start = segments.len().saturating_sub(3);
    segments[start..].join("/")
}

/// `main-A1B2C3D4.js` becomes `main.js`.
///
/// The hash segment must contain a digit, so a legitimate name like
/// `main-production.js` survives.
fn strip_content_hash(path: &str) -> String {
    static HASH: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"[-.]([0-9A-Za-z]{8,})(\.[0-9A-Za-z]+)$").expect("valid regex")
    });

    let Some(caps) = HASH.captures(path) else {
        return path.to_string();
    };
    let candidate = &caps[1];
    if !candidate.chars().any(|c| c.is_ascii_digit()) {
        return path.to_string();
    }

    let full = caps.get(0).expect("group 0 always present");
    let mut out = String::with_capacity(path.len());
    out.push_str(&path[..full.start()]);
    out.push_str(&caps[2]);
    out
}

/// Collapse the variable parts of a message so two reports that differ only by
/// an id, number, or quoted value group together.
#[must_use]
pub fn normalize_message(message: &str) -> String {
    static URL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[a-zA-Z][a-zA-Z0-9+.-]*://\S+").expect("valid regex"));
    static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").expect("valid regex")
    });
    static UUID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
            .expect("valid regex")
    });
    static QUOTED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#""[^"]*"|'[^']*'"#).expect("valid regex"));
    static HEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b[0-9a-f]{8,}\b").expect("valid regex"));
    static NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").expect("valid regex"));

    // Order matters: broader patterns first, so their contents are not partly
    // consumed by a narrower one. The placeholders contain no digits or hex
    // characters, so later passes leave them alone.
    let text = URL.replace_all(message.trim(), "<url>");
    let text = EMAIL.replace_all(&text, "<email>");
    let text = UUID.replace_all(&text, "<uuid>");
    let text = QUOTED.replace_all(&text, "<str>");
    let text = HEX.replace_all(&text, "<hex>");
    let text = NUM.replace_all(&text, "<num>");

    truncate_chars(&collapse_whitespace(&text), MAX_MESSAGE_KEY_LEN)
}

/// A short human label for an issue: the top in-app frame, if there is one.
#[must_use]
pub fn culprit(frames: &[Frame]) -> Option<String> {
    select_frames(frames).first().map(|frame| {
        let function = frame
            .function
            .as_deref()
            .map(normalize_function)
            .filter(|f| !f.is_empty());
        let file = frame.file.as_deref().map(normalize_file);
        match (function, file) {
            (Some(f), Some(file)) => format!("{f} ({file})"),
            (Some(f), None) => f,
            (None, Some(file)) => file,
            (None, None) => "<unknown>".to_string(),
        }
    })
}

/// Remove balanced `<...>` sections, which carry generic arguments that vary
/// without changing the call site.
fn strip_generics(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth: usize = 0;
    for ch in input.chars() {
        match ch {
            '<' => depth += 1,
            // A `>` with no matching `<` is part of something else (`->`), keep it.
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    // `-` of a `->` survives above; tidy the leftover arrow.
    out.trim().to_string()
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Truncate on a character boundary — messages are arbitrary UTF-8.
fn truncate_chars(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(function: &str, file: &str, line: u32) -> Frame {
        Frame {
            function: Some(function.to_string()),
            file: Some(file.to_string()),
            line: Some(line),
            column: None,
            in_app: true,
        }
    }

    fn input<'a>(
        frames: &'a [Frame],
        error_type: &'a str,
        message: &'a str,
    ) -> FingerprintInput<'a> {
        FingerprintInput {
            source: Source::App,
            error_type,
            message,
            frames,
            client_fingerprint: None,
            call_site: None,
        }
    }

    // The property this whole module exists to guarantee.
    #[test]
    fn line_numbers_do_not_affect_grouping() {
        let a = vec![frame("Foo.bar", "/src/app/foo.ts", 12)];
        let b = vec![frame("Foo.bar", "/src/app/foo.ts", 340)];
        assert_eq!(
            fingerprint(&input(&a, "TypeError", "x is not a function")),
            fingerprint(&input(&b, "TypeError", "x is not a function")),
        );
    }

    #[test]
    fn source_namespaces_the_key() {
        let frames = vec![frame("handle", "/src/app/foo.ts", 1)];
        let mut app = input(&frames, "TypeError", "boom");
        let mut api = input(&frames, "TypeError", "boom");
        app.source = Source::App;
        api.source = Source::Api;
        assert_ne!(fingerprint(&app), fingerprint(&api));
    }

    #[test]
    fn bundle_content_hashes_are_stripped() {
        let a = vec![frame(
            "Foo",
            "https://app.example/assets/main-A1B2C3D4.js",
            1,
        )];
        let b = vec![frame(
            "Foo",
            "https://app.example/assets/main-99887766.js",
            9,
        )];
        assert_eq!(
            fingerprint(&input(&a, "TypeError", "boom")),
            fingerprint(&input(&b, "TypeError", "boom")),
        );
    }

    #[test]
    fn a_hashlike_word_without_digits_is_not_stripped() {
        // `production` is 10 chars but has no digit — it is a real name.
        assert_eq!(
            normalize_file("/assets/main-production.js"),
            "assets/main-production.js"
        );
    }

    #[test]
    fn rust_symbol_hashes_and_closures_are_stripped() {
        assert_eq!(
            normalize_function("erno::sync::pull::h0123456789abcdef"),
            "erno::sync::pull"
        );
        assert_eq!(
            normalize_function("erno::jobs::worker::run::{{closure}}"),
            "erno::jobs::worker::run"
        );
        assert_eq!(
            normalize_function("erno::a::run::{closure#0}"),
            "erno::a::run"
        );
    }

    #[test]
    fn generic_arguments_are_stripped() {
        assert_eq!(
            normalize_function("core::ptr::drop_in_place<Vec<Foo>>"),
            "core::ptr::drop_in_place"
        );
        assert_eq!(normalize_type("DbErr<Postgres>"), "DbErr");
    }

    #[test]
    fn cargo_registry_paths_lose_their_version() {
        let a = normalize_file(
            "/home/u/.cargo/registry/src/index.crates.io-6f17d22bba15001f/sqlx-core-0.8.2/src/pool.rs",
        );
        let b = normalize_file(
            "/home/u/.cargo/registry/src/index.crates.io-6f17d22bba15001f/sqlx-core-0.9.0/src/pool.rs",
        );
        assert_eq!(a, b);
        assert_eq!(a, "src/pool.rs");
    }

    #[test]
    fn stackless_messages_collapse_variable_parts() {
        let none: Vec<Frame> = vec![];
        let a = input(
            &none,
            "erno::admin",
            "failed to load user 550e8400-e29b-41d4-a716-446655440000",
        );
        let b = input(
            &none,
            "erno::admin",
            "failed to load user 6ba7b810-9dad-11d1-80b4-00c04fd430c8",
        );
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn normalize_message_placeholders() {
        assert_eq!(
            normalize_message("user 42 not found"),
            "user <num> not found"
        );
        assert_eq!(
            normalize_message("mail to a.b@example.com bounced"),
            "mail to <email> bounced"
        );
        assert_eq!(
            normalize_message("GET https://x.test/a?b=1 failed"),
            "GET <url> failed"
        );
        assert_eq!(
            normalize_message(r#"key "abc" missing"#),
            "key <str> missing"
        );
        assert_eq!(
            normalize_message("token deadbeefcafe1234 bad"),
            "token <hex> bad"
        );
    }

    #[test]
    fn different_messages_still_group_apart() {
        let none: Vec<Frame> = vec![];
        let a = input(&none, "erno::admin", "failed to load user 1");
        let b = input(&none, "erno::admin", "database connection refused");
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn call_site_participates_when_there_is_no_stack() {
        let none: Vec<Frame> = vec![];
        let mut a = input(&none, "erno::admin", "boom");
        let mut b = input(&none, "erno::admin", "boom");
        a.call_site = Some("src/admin/handlers.rs");
        b.call_site = Some("src/admin/service.rs");
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn vendor_frames_are_skipped_when_app_frames_exist() {
        let frames = vec![
            frame("push", "/node_modules/rxjs/Subject.js", 1),
            frame("Foo.bar", "/src/app/foo.ts", 10),
        ];
        let only_app = vec![frame("Foo.bar", "/src/app/foo.ts", 10)];
        assert_eq!(
            fingerprint(&input(&frames, "TypeError", "boom")),
            fingerprint(&input(&only_app, "TypeError", "boom")),
        );
    }

    #[test]
    fn vendor_frames_are_used_when_they_are_all_there_is() {
        let frames = vec![frame("push", "/node_modules/rxjs/Subject.js", 1)];
        assert!(!select_frames(&frames).is_empty());
    }

    #[test]
    fn only_the_top_five_frames_participate() {
        let mut base: Vec<Frame> = (0..5)
            .map(|i| frame("f", &format!("/src/a{i}.ts"), 1))
            .collect();
        let mut extended = base.clone();
        extended.push(frame("deep", "/src/z.ts", 1));
        assert_eq!(
            fingerprint(&input(&base, "E", "m")),
            fingerprint(&input(&extended, "E", "m")),
        );
        // ...but a change within the top five does split.
        base[0] = frame("other", "/src/a0.ts", 1);
        assert_ne!(
            fingerprint(&input(&base, "E", "m")),
            fingerprint(&input(&extended, "E", "m")),
        );
    }

    #[test]
    fn client_fingerprint_overrides_but_stays_namespaced() {
        let frames = vec![frame("Foo", "/src/a.ts", 1)];
        let custom = vec!["checkout".to_string(), "declined".to_string()];
        let mut a = input(&frames, "TypeError", "boom");
        let mut b = input(&frames, "RangeError", "totally different");
        a.client_fingerprint = Some(&custom);
        b.client_fingerprint = Some(&custom);
        assert_eq!(fingerprint(&a), fingerprint(&b));

        // Same override from a different source must not collide.
        let mut c = input(&frames, "TypeError", "boom");
        c.client_fingerprint = Some(&custom);
        c.source = Source::Api;
        assert_ne!(fingerprint(&a), fingerprint(&c));
    }

    #[test]
    fn client_fingerprint_is_capped_and_trimmed() {
        let frames: Vec<Frame> = vec![];
        let many: Vec<String> = (0..20).map(|i| format!("part{i}")).collect();
        let mut i = input(&frames, "E", "m");
        i.client_fingerprint = Some(&many);
        let parts = fingerprint_parts(&i);
        // source + 8 capped parts
        assert_eq!(parts.len(), MAX_CLIENT_PARTS + 1);

        let long = vec!["x".repeat(500)];
        let mut j = input(&frames, "E", "m");
        j.client_fingerprint = Some(&long);
        assert_eq!(
            fingerprint_parts(&j)[1].chars().count(),
            MAX_CLIENT_PART_LEN
        );
    }

    #[test]
    fn empty_client_fingerprint_falls_through_to_normal_grouping() {
        let frames = vec![frame("Foo", "/src/a.ts", 1)];
        let empty: Vec<String> = vec![String::new(), "   ".to_string()];
        let mut a = input(&frames, "TypeError", "boom");
        a.client_fingerprint = Some(&empty);
        assert_eq!(
            fingerprint(&a),
            fingerprint(&input(&frames, "TypeError", "boom"))
        );
    }

    #[test]
    fn fingerprint_is_64_hex_chars() {
        let frames = vec![frame("Foo", "/src/a.ts", 1)];
        let fp = fingerprint(&input(&frames, "TypeError", "boom"));
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn culprit_prefers_app_frames() {
        let frames = vec![
            frame("push", "/node_modules/rxjs/Subject.js", 1),
            frame("Foo.bar", "/src/app/foo.ts", 10),
        ];
        assert_eq!(
            culprit(&frames),
            Some("Foo.bar (src/app/foo.ts)".to_string())
        );
        assert_eq!(culprit(&[]), None);
    }

    #[test]
    fn multibyte_messages_truncate_safely() {
        let long = "日本語".repeat(200);
        let out = normalize_message(&long);
        assert_eq!(out.chars().count(), MAX_MESSAGE_KEY_LEN);
    }

    #[test]
    fn in_app_classification() {
        assert!(is_in_app(Some("/src/app/foo.ts")));
        assert!(!is_in_app(Some("/app/node_modules/x/y.js")));
        assert!(!is_in_app(Some(
            "/home/u/.cargo/registry/src/i/x-1.0/src/a.rs"
        )));
        assert!(!is_in_app(None));
    }
}
