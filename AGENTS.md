# Erno

Rust/Axum SaaS infrastructure: auth, jobs, billing, sync, storage.

## Layout

| Dir | What |
|-----|------|
| `api/` | Rust library |
| `app/` | Angular library (`erno-angular`) |
| `admin/` | Operator Angular app |
| `cli/` | `erno` CLI |
| `docs/` | Astro docs (`cd docs && npm run dev`) |
| `monitoring/` | Separate collector deployment |

## Build

`api/`, `cli/` and `monitoring/` share one cargo workspace. `app/`, `admin/`, `monitoring/ui` and `docs/` are npm projects.

```sh
./build.sh              # build everything
./build.sh api cli      # selected parts
./build.sh test         # Rust tests (needs PostgreSQL)
./build.sh check        # fmt + clippy -D warnings
```

Shared crate versions live in root `Cargo.toml` `[workspace.dependencies]`. Add a dependency there when a second crate uses it.

**Never `cargo test --workspace` from the root.** Monitoring tests must run single-threaded: `./build.sh test` or `cd monitoring && cargo test`.
