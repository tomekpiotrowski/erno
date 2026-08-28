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
| `error-reporting-types/` | The error-reporting contract `api/` and `monitoring/` share |
| `monitoring/` | The collector: its own crate, its own deployment |

## Build

`api/`, `cli/`, `error-reporting-types/` and `monitoring/` share one cargo workspace. `app/`, `admin/`, `monitoring/ui` and `docs/` are npm projects.

`monitoring/` depends on `api/`, never the other way round. The collector watches applications and must not ship with them, so nothing in `api/` may reach into it — the only thing both sides share is `error-reporting-types/`.

```sh
./build.sh              # build everything
./build.sh api cli      # selected parts
./build.sh test         # Rust tests (needs PostgreSQL)
./build.sh check        # fmt + clippy -D warnings
```

Shared crate versions live in root `Cargo.toml` `[workspace.dependencies]`. Add a dependency there when a second crate uses it.

**Never `cargo test --workspace` from the root.** Monitoring tests must run single-threaded: `./build.sh test` or `cd monitoring && cargo test`.
