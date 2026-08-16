# Erno CLI

The `erno` binary — developer tooling for the Erno framework. All commands below run from this directory (`cli/`).

## Building & running

```sh
cargo build                              # debug build
cargo run -- <command>                   # run without installing
cargo install --path .                   # install globally as `erno`
```

## Commands

| Command | Description |
|---------|-------------|
| `erno setup` | Interactive wizard — writes `~/.erno/config.toml` with PostgreSQL admin credentials |
| `erno doctor` | Checks the local environment: Rust, Node, Angular CLI, PostgreSQL, `~/.erno/config.toml`, admin DB access |
| `erno new <name>` | Scaffolds a full-stack Erno project (Rust API + Ionic app + Astro www) |
| `erno dev` | Starts api + app + www dev servers, readiness banner, `--ios`/`--android` live reload |
| `erno dev` | Also starts Prometheus (if installed) and `admin/` on :4300 |
| `erno deploy init` | Scaffolds Docker/Helm deploy files; generates admin password hash for production |
| `erno deploy install` | Installs a chart version to the cluster (`helm secrets upgrade --install`) |
| `erno build` | Builds every package declared in `erno.toml`, in declaration order |
| `erno lint` | Format-checks, lints, and typechecks every package; `--fix` applies fixes |
| `erno test` | Runs each package's test steps, then Playwright e2e |

Narrative docs for the CLI live in `docs/src/content/docs/cli/` (`index.md`, `deploy.md`).

## The package manifest: `erno.toml`

`build`, `lint`, and `test` all read one file in the project root. Each `[[package]]` declares its own steps per phase, and **declaration order is execution order** — that is how build dependency order is expressed, which is why there is no dependency graph and no parallelism.

```toml
[[package]]
name = "puzzles"
dir  = "puzzles"

  [[package.build]]
  command = "./build.sh"

  [[package.lint]]
  command = "cargo"
  args    = ["fmt", "--check"]
  fix     = ["fmt"]

  [[package.test]]
  command = "cargo"
  args    = ["test"]
```

| Package key | Meaning |
|-------------|---------|
| `name`, `dir` | Required. `dir` is relative to the project root. |
| `default` | `false` means opt in with `--package <name>` or `--all`. Defaults to `true`. |
| `database` | Ensure the test database exists before this package's test phase. |
| `kind` | Only `"e2e"` is recognised — the CLI runs its own port-allocating orchestration and ignores declared test steps. |

| Step key | Meaning |
|----------|---------|
| `command` | Required. Run with `dir` as the working directory. |
| `args` | Defaults to `[]`. |
| `fix` | Lint only: the argument vector substituted under `--fix`. A step without `fix` runs its check form unchanged. |
| `default` | `false` makes the step itself opt-in — used for slow guards and optional bundles. |

Unknown keys are rejected, so typos fail loudly. When `erno.toml` is absent the CLI falls back to the conventional layout (`api/Cargo.toml`, `app/package.json`, `e2e/playwright.config.ts`), so a freshly scaffolded project needs no manifest.

### `erno new` options

| Flag | Default | Description |
|------|---------|-------------|
| `--path <dir>` | current directory | Where to create the project directory |
| `--erno-path <path>` | git reference | Path to a local erno repo root or its `api/` checkout; also packs `app/dist/erno-angular` and wires the tarball into the generated app |

Without `--erno-path` the generated `api/Cargo.toml` references:
```toml
erno = { git = "https://github.com/tomekpiotrowski/erno" }
```
and `app/package.json` references `"erno-angular": "^0.0.1"`.

With `--erno-path /path/to/erno`:
```toml
erno = { path = "/path/to/erno/api" }
```
```json
"erno-angular": "file:/path/to/erno/app/dist/erno-angular-0.0.1.tgz"
```

## Global config: `~/.erno/config.toml`

Created by `erno setup`. Required by `erno doctor` and `erno new`.

```toml
[postgres]
admin_url = "postgres://erno:erno@localhost:5432/postgres"
```

The admin user must have `CREATEDB` privilege:
```sql
ALTER USER erno CREATEDB;
```

## Key source files

| File | Responsibility |
|------|---------------|
| `src/main.rs` | CLI entry point — clap command definitions and dispatch |
| `src/global_config.rs` | `~/.erno/config.toml` read/write via the `config` crate |
| `src/commands/setup.rs` | Interactive config writer; validates admin connection before saving |
| `src/commands/doctor.rs` | Environment checks — each returns a `CheckResult` (Pass/Warn/Fail) |
| `src/commands/new.rs` | Project scaffolding — inline templates, directory creation, database creation |
| `src/commands/dev/` | `erno dev` — process multiplexer, readiness banner, quiet logs |
| `src/commands/packages.rs` | `erno.toml` parsing, package selection, and the sequential phase runner shared by build/lint/test |
| `src/commands/build.rs` | `erno build` — runs the `build` phase |
| `src/commands/lint.rs` | `erno lint` — runs the `lint` phase, with `--fix` |
| `src/commands/test.rs` | `erno test` — the `test` phase, plus test-database setup and e2e orchestration |

## Architecture notes

- **No dependency on `api/`**: the CLI does not depend on the `erno` library crate. Admin uses `reqwest` + `ratatui` as an HTTP client. Keeping it decoupled avoids version skew and circular concerns.
- **One manifest, three commands**: `build`, `lint`, and `test` differ only in which phase they run and whether `--fix` applies. The shared engine lives in `packages.rs`; the command modules are thin. `test.rs` is the only one that needs more, because the e2e package is orchestrated rather than shelled out — it passes a callback that `run_phase` gives first refusal on each package.
- **Templates are inline strings**: `new.rs` holds all scaffold templates as Rust string constants/functions. `{{name}}` is substituted via `.replace()` — no template engine dependency.
- **`erno_migrations()` helper**: scaffolded apps call `erno::database::migrations::erno_migrations()` in their `Migrator` to include all built-in framework migrations (users, jobs, sync, billing, storage) before their own.
- **Database creation**: `erno new` connects with the admin URL from `~/.erno/config.toml` and issues `CREATE DATABASE` for `<name>_development` and `<name>_test`.

## Adding a new command

1. Create `src/commands/<command>.rs` with a `pub async fn handle_<command>(...)` function.
2. Add `pub mod <command>;` to `src/commands/mod.rs`.
3. Add a variant to the `Commands` enum in `src/main.rs` and dispatch it in `main()`.
