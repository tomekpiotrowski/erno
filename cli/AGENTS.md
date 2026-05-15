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
| `erno new <name>` | Scaffolds a full-stack Erno project (Rust API + Angular app) |

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

## Architecture notes

- **No dependency on `api/`**: the CLI uses only `std`, `clap`, `tokio-postgres`, `rand`, `base64`, `config`, and `dirs`. Keeping it decoupled lets it compile fast and avoids circular concerns.
- **Templates are inline strings**: `new.rs` holds all scaffold templates as Rust string constants/functions. `{{name}}` is substituted via `.replace()` — no template engine dependency.
- **`erno_migrations()` helper**: scaffolded apps call `erno::database::migrations::erno_migrations()` in their `Migrator` to include all built-in framework migrations (users, jobs, sync, billing, storage) before their own.
- **Database creation**: `erno new` connects with the admin URL from `~/.erno/config.toml` and issues `CREATE DATABASE` for `<name>_development` and `<name>_test`.

## Adding a new command

1. Create `src/commands/<command>.rs` with a `pub async fn handle_<command>(...)` function.
2. Add `pub mod <command>;` to `src/commands/mod.rs`.
3. Add a variant to the `Commands` enum in `src/main.rs` and dispatch it in `main()`.
