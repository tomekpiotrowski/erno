# Erno API

Rust library. Commands run from `api/`.

```sh
cargo build --all-features
cargo test --all-features    # PostgreSQL at postgres://erno:erno@localhost/erno (config/test.toml)
cargo clippy --all-features
cargo fmt
```

`test-utils` is required to compile tests. Rate limiting and email sending are off in test.

Consuming apps boot via `app.rs`. Config is TOML per environment in `config/`.

Narrative docs: `docs/src/content/docs/api/`. Update the matching page when you change a public API, config key, or observable behaviour.
