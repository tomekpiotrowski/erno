# Erno

Rust/Axum SaaS infrastructure library — batteries-included auth, jobs, billing, sync, storage, and an offline-first sync engine.

## Monorepo layout

| Directory | What it is |
|-----------|------------|
| `api/`    | Main Rust library crate — see `api/AGENTS.md` for development instructions |
| `app/`    | Angular library (`erno-angular`) for Ionic web + mobile — see `app/AGENTS.md` |
| `cli/`    | `erno` CLI binary — scaffolding, environment checks — see `cli/AGENTS.md` |
| `docs/`   | Astro documentation site |
| `monitoring/` | Monitoring collector — errors, metrics, traces and logs, deployed separately from the app it watches. See `monitoring/AGENTS.md` |

## Building everything

`api/`, `cli/` and `monitoring/` are members of one cargo workspace, declared in the root `Cargo.toml`; `app/`, `admin/`, `monitoring/ui` and `docs/` are npm projects. `./build.sh` is the single entry point across all of them:

```sh
./build.sh              # build api, cli, app, admin, monitoring, and docs
./build.sh api cli      # build only those parts
./build.sh test         # Rust test suites (api tests require PostgreSQL)
./build.sh check        # cargo fmt --all --check + clippy --workspace -D warnings
./build.sh help         # list every target
```

Shared dependency versions live in `[workspace.dependencies]` in the root
`Cargo.toml`. Members inherit with `serde = { workspace = true }` and add extra
features where they need them — `tokio = { workspace = true, features = ["fs"] }`.
Add a dependency there whenever a second crate starts using it, so the three
crates cannot drift apart again.

**The monitoring test suite must run single-threaded.** Its tests issue
table-wide statements that deadlock against each other's uncommitted rows.
`monitoring/.cargo/config.toml` sets `RUST_TEST_THREADS=1`, and cargo only finds
it when invoked from inside `monitoring/` — so run the suite via `./build.sh
test` or `cd monitoring && cargo test`, never `cargo test --workspace` from the
root. A guard in the suite fails loudly if that happens anyway.

Per-directory instructions below still apply when working inside one part.

## API (Rust)

`api/` is the Rust library crate — auth, jobs, billing, sync, storage, and more. See `api/AGENTS.md` for build instructions, module reference, and architecture notes.

## App (Angular)

`app/` contains `erno-angular` — an Angular 22 library that Ionic apps consume for web and mobile. See `app/AGENTS.md` for build instructions, service reference, and architecture notes.

## CLI

`cli/` contains the `erno` binary. Install with `cargo install --path cli` or `cargo install erno-cli`. See `cli/AGENTS.md` for command reference and development instructions.

## Monitoring

`monitoring/` is a separate deployment — its own binary, database, and operator
console — that collects errors from the API, Angular apps, and the admin panel.
It runs on infrastructure separate from the application, so it survives the
outages it exists to report. See `monitoring/AGENTS.md`.

## Docs (Astro)

```sh
cd docs
npm run dev        # dev server
npm run build      # production build
```
