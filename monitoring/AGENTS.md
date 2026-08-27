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

Collector logic lives in `api/src/error_reporting/collector/`. Collector migrations are not in `erno_migrations()`.

Narrative docs: `docs/src/content/docs/monitoring/`. Update the matching page when you change the ingest contract, config keys, grouping rules, or OTLP / Tempo / Loki paths.
