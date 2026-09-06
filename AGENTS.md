# Erno

Rust/Axum SaaS infrastructure: auth, jobs, billing, sync, storage.

## Layout

| Dir | What |
|-----|------|
| `api/` | Rust library |
| `app/` | Angular library (`erno-angular`) |
| `admin/` | Operator Angular app |
| `cli/` | `erno` CLI |
| `docs/` | Astro docs (`cd docs && bun run dev`) |
| `error-reporting-types/` | The error-reporting contract this repo and the collector share |

## Build

`api/`, `cli/` and `error-reporting-types/` share one cargo workspace. `app/`, `admin/` and `docs/` are Bun projects.

Use Bun 1.4.0 for JavaScript dependencies and scripts. Use `bun run test` for
the existing test runners and `bun install --frozen-lockfile` in automated builds.
Commit `bun.lock`; Node.js remains required by Angular and Ionic.

The monitoring application lives in its own repository (`erno-monitoring`). It depends on this one; nothing here may depend on it. The only thing both sides share is `error-reporting-types/` — what crosses the wire between an application and the collector watching it. Generated apps do not contain a collector: one deployment watches every Erno app in an organisation. The CLI must not encode erno-monitoring's store (image, ports, schema version); that topology lives in `erno-monitoring`.

```sh
./build.sh              # build everything
./build.sh api cli      # selected parts
./build.sh test         # Rust tests (needs PostgreSQL)
./build.sh check        # fmt + clippy -D warnings
```

Shared crate versions live in root `Cargo.toml` `[workspace.dependencies]`. Add a dependency there when a second crate uses it.

`./build.sh test` runs every suite. The collector's tests live in the erno-monitoring repository, not here.
