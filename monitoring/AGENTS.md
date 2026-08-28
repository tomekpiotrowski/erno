# Erno Monitoring

Separate deployment that watches an Erno application. Commands run from `monitoring/`.

```sh
cargo build
cargo test                 # PostgreSQL at postgres://erno:erno@localhost/erno_monitoring_test
cargo clippy -- -D warnings
cargo fmt
cargo run -- serve         # collector on :3001
cd ui && npm start         # operator console on :4400
```

This is a different database from the API tests. Tests run single-threaded (`.cargo/config.toml`). Never `cargo test --workspace` from the repo root. Every test must boot through `setup_with` so the single-thread guard runs.

Collector logic lives in `src/collector/`, with `src/fingerprint.rs` (grouping) and `src/scrub.rs` (redaction) beside it. Collector migrations are not in `erno_migrations()`.

`src/lib.rs` is the crate; `src/main.rs` only calls `boot()`. This crate depends on `erno`, never the reverse — the only thing both share is the `erno-error-reporting-types` crate, which holds what crosses the wire.

Narrative docs: `docs/src/content/docs/monitoring/`. Update the matching page when you change the ingest contract, config keys, grouping rules, or OTLP / Tempo / Loki paths.
