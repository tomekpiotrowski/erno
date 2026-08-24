---
title: CLI Overview
description: The erno CLI — project scaffolding, environment verification, and developer tooling
sidebar:
  order: 0
---

The `erno` CLI is the recommended way to create and manage Erno projects. It scaffolds full-stack projects, verifies your local environment, and stores shared configuration such as your PostgreSQL admin connection.

## Installation

```sh
cargo install --path cli      # from the erno repo
# or, once published:
cargo install erno-cli
```

## Commands

| Command | Description |
|---------|-------------|
| [`erno setup`](#setup) | Configure `~/.erno/config.toml` (PostgreSQL admin credentials) |
| [`erno doctor`](#doctor) | Verify that your environment is ready to develop Erno apps |
| [`erno new <name>`](#new) | Scaffold a new full-stack Erno project |
| [`erno dev`](#dev) | Start the API, app, www, Prometheus, and admin SPA |
| [`erno build`](#build) | Build every package, in dependency order |
| [`erno lint`](#lint) | Format-check, lint, and typecheck every package |
| [`erno test`](#test) | Run each package's tests, then e2e |
| [`erno upgrade`](/cli/upgrade/) | Inventory and update Erno-managed packages |
| [`erno deploy`](/cli/deploy/) | Scaffold Docker/deploy files and install releases |

`build`, `lint`, and `test` read one manifest — [`erno.toml`](#the-package-manifest) in the project root. `erno dev` reads the same file for optional `[[package.dev]]` children.

---

## Output conventions

Every command shares one output style.

```text
==> Section header
  ok    a passing row
  warn  something to look at
  fail  something broken
        an explanation or fix
error: the command failed
```

Status markers are plain words — `ok`, `warn`, `fail`, `error:` — coloured green, yellow, and red. They are never emoji, so columns line up on every terminal and nothing is lost when colour is off.

**stdout carries the output of the tools the CLI runs; stderr carries everything the CLI says about itself.** Headers, rows, banners, prompts, warnings, and errors all go to stderr. So `erno doctor > report.txt` writes an empty file (the report is on stderr), while `erno build 2>/dev/null` shows only what cargo and npm printed to stdout. The pinned `erno dev` banner is a stderr-only affair too, so redirecting either stream behaves exactly as it always has.

### Global flags

These work on either side of the subcommand — `erno -q build` and `erno build -q` are the same.

| Flag | Effect |
|------|--------|
| `--no-color` | Disable ANSI colour. `NO_COLOR` and `CLICOLOR_FORCE` are also honoured. |
| `--quiet`, `-q` | Print only warnings, errors, and results. Never hides a failure. |
| `--verbose`, `-v` | Print more detail. In `dev`, stream every child log line; in `deploy init`, list every generated file. |

Colour is enabled when stderr is a terminal that understands ANSI, and disabled when it is piped or redirected — no flag needed for CI.

## dev

```sh
erno dev
erno dev --verbose
erno dev --api
erno dev --app --www
erno dev --no-www
erno dev --seed
erno dev --open
erno dev --ios
erno dev --android
erno dev --package vision
erno dev --all
```

Starts the project’s dev servers (`api/` + `app/`, plus `www/` when present). Walks up from the current directory looking for `api/Cargo.toml`, so you can run it from `api/`, `app/`, or any subdirectory. Child tools are told to keep colour even though their stdout is piped — and told to drop it when you pass `--no-color`.

By default only errors and ready events are printed; the full multiplex is written to `.erno/dev.log`. Pass the global `--verbose` (or `-v`) to stream every child line, prefixed by service (`[api]`, `[app]`, `[www]`). The `.erno/dev.log` copy is always uncoloured, so it greps cleanly.

`--api`, `--app`, and `--www` start only the services you name (combine them). `--no-www` skips the marketing site when you want the default API + app pair. `--api` does not require an `app/` directory.

`--package <name>` (repeatable) and `--all` start extra long-running processes declared as `[[package.dev]]` in `erno.toml`, **in addition to** the usual api/app/www selection. A package with `default = false` is not started by plain `erno dev`. Naming a package does not pull a `default = false` `[[package.dev]]` step unless `--all` is also passed. `--open` still prefers www, then the app, then the API — extra URLs are not opened.

`--seed` inserts a verified demo user (`dev@example.com` / `password`) if it is missing. An empty `users` table is seeded automatically on first run so login works without walking through email verification.

Override the demo account in `api/config/development.toml`. A `[seed]` user is also created when other users already exist:

```toml
[seed]
email = "dev@example.com"
password = "password"
```

`--open` opens one browser tab once a service is ready, preferring www, then the app, then the API.

`--ios` / `--android` start the API plus `ionic cap run` with live reload on the machine’s LAN IP. The app is rewritten for the session to call `http://<lan>:api-port`, extra CORS origins are passed to the API as `ERNO_DEV_CORS_ORIGINS`, and the original environment file is restored on exit.

The native project must already exist (`cd app && npx cap add android`), and the Ionic CLI is taken from `app/node_modules/.bin/ionic`, then `PATH`, and only then fetched with `npx --yes @ionic/cli`. Projects scaffolded by `erno new` carry `@ionic/cli` as a devDependency.

The device is resolved before anything starts — `ionic` itself runs non-interactively, because a child of `erno dev` cannot answer a prompt. A single attached device or emulator is used automatically; when several are attached, `erno dev` lists them and asks for `--target <id>`:

```sh
erno dev --android --target emulator-5554
```

`erno dev` prints a status banner with each service URL and probes them until they respond:

| Surface | Probe | Default URL (overridden by project config) |
|---------|-------|-------------|
| API | `GET /readiness` (`/liveness` while migrating) | `[server].port` / `api_url` in `api/config/development.toml` |
| Product app | HTTP | `app_url` or `angular.json` serve port (4200) |
| Marketing | HTTP | `--port` in `www/package.json` `dev` script (4321) |
| Prometheus | `GET /-/ready` | `http://localhost:9090` |
| Extra `[[package.dev]]` | HTTP on the declared `url` | From `erno.toml` |

A second `erno dev` in the same project is rejected via `.erno/dev.lock` (stale locks from a crashed session are replaced). When the API is running the banner also lists Prometheus (`http://localhost:9090`), the admin SPA (`http://localhost:4300`, password `admin`), `/dev/emails`, and `/dev/jobs`. Newly captured mock emails are printed as `[mail] subject → to`. In an interactive terminal the banner is pinned to the bottom of the screen and updated in place as services come up, with log output scrolling above it; on Ctrl+C the last copy is left in the scrollback. When output is piped, under `--no-color`, `--quiet`, or `--verbose`, on a terminal too small to hold the banner, or with `ERNO_STICKY=0`, it is printed once instead and each later state change (`starting` → `ready`) is reported as a single row naming the service. If one process exits, it is restarted (with backoff) without taking the others down. The API is rebuilt automatically when `api/` source files change (no `cargo-watch` needed). Ctrl+C sends SIGTERM, then SIGKILL after two seconds. Prometheus is required when the API is started (`prometheus` must be on `PATH`). Pass `--no-prometheus` to skip — the banner then omits `prom`. A missing binary is an error, not a silent skip.

Before spawning anything, `erno dev` checks that PostgreSQL is running (when the API is selected), that `prometheus` is on `PATH` (unless `--no-prometheus`), and that each selected service’s port is free. If a port is held by a leftover `cargo`/`node`/`erno` process, it offers to kill it.

---

## The package manifest

`erno.toml` in the project root declares every package once. `erno build`, `erno lint`, and `erno test` are the same runner over three different phases.

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

  [[package.lint]]
  command = "cargo"
  args    = ["clippy", "--all-targets", "--", "-D", "warnings"]
  fix     = ["clippy", "--all-targets", "--fix", "--allow-dirty"]

  [[package.test]]
  command = "cargo"
  args    = ["test"]

  # A slow guard nobody wants on every run.
  [[package.test]]
  command = "cargo"
  args    = ["test", "--release", "--", "--ignored"]
  default = false

[[package]]
name = "app"
dir  = "app"

  [[package.build]]
  command = "npm"
  args    = ["run", "build"]
```

**Declaration order is execution order.** Packages run top to bottom, sequentially, which is how build dependency order is expressed — put the package that generates an artifact above the package that consumes it. There is no dependency graph and no parallelism.

| Package key | Default | Meaning |
|-------------|---------|---------|
| `name` | — | Required. The selector for `--package`. |
| `dir` | — | Required. Working directory for every step, relative to the project root. |
| `default` | `true` | `false` means opt in with `--package <name>` or `--all`. |
| `database` | `false` | Ensure the test database exists before this package's test phase. |
| `kind` | — | Only `"e2e"` is recognised. The CLI orchestrates that package itself and ignores its declared test steps. |
| `[[package.dev]]` | — | At most one long-running process for `erno dev`. Required `command` and `url`; optional `args`, `default`. Not a `build`/`lint`/`test` phase. |

| Step key | Default | Meaning |
|----------|---------|---------|
| `command` | — | Required. |
| `args` | `[]` | Arguments to `command`. |
| `fix` | `[]` | Lint only: substituted for `args` under `--fix`. A step without `fix` runs its check form unchanged. |
| `default` | `true` | `false` makes the step itself opt-in, without splitting the package. |

Unknown keys are an error, so a typo like `defualt = false` fails loudly instead of being silently ignored.

If `erno.toml` is missing, the CLI falls back to the layout `erno new` scaffolds — `api/Cargo.toml`, `app/package.json`, and `e2e/playwright.config.ts` — so a new project works without one.

Every command walks up from the current directory to find the project root, so they can be run from any subdirectory. `node_modules` is installed automatically for any package that has a `package.json` but no `node_modules`.

---

## build

```sh
erno build
erno build --package puzzles
erno build --all
erno build --api
erno build --fail-fast
```

Runs each selected package's `build` steps in declaration order. A package with no `build` steps is skipped silently.

---

## lint

```sh
erno lint
erno lint --fix
erno lint --package api
erno lint --all
```

Runs each selected package's `lint` steps. `--fix` swaps in each step's `fix` arguments — typically `cargo fmt` in place of `cargo fmt --check`, and `clippy --fix` in place of `clippy -D warnings`. Steps that define no `fix` still run in check mode, so `--fix` never silently skips a check.

Exits non-zero if any step fails.

---

## test

```sh
erno test
erno test --api
erno test --app
erno test --e2e
erno test --no-e2e
erno test --package puzzles
erno test --api -- health
```

Runs each selected package's `test` steps. Ensures the test database from `api/config/test.toml` exists first when any selected package sets `database = true` or is the e2e package. The e2e package is special-cased: the CLI allocates two free ports, boots the API against the test database, waits for `/liveness`, runs Playwright, and tears the API down. See [Testing](/api/testing/).

---

## Selecting packages

These flags are shared by `build`, `lint`, and `test`:

| Flag | Effect |
|------|--------|
| `--package <name>` | Select this package. Repeatable. Required for a package marked `default = false`. |
| `--api`, `--app`, `--e2e` | Shorthand for `--package api` / `app` / `e2e`. |
| `--all` | Include packages *and* steps marked `default = false`. |
| `--no-e2e` | Drop the e2e package from the selection. |
| `--fail-fast` | Stop after the first failing package. |
| `-- <args>` | Forwarded to the selected package's steps. Requires exactly one package. |

With no flags, every package with `default = true` runs, and within them every step with `default = true`.

`--all` is the only thing that pulls in `default = false` *steps*. Naming a package selects the package, not its slow extras — `erno test --package puzzles` runs the test suite without also starting the multi-minute release guard declared alongside it. A `default = false` *package*, on the other hand, is selected by naming it or by `--all`.

Each command prints a per-package `ok` / `fail` summary and exits non-zero if any package failed:

```text
==> api
[api]    Compiling erno v0.1.0

==> app
[app] > app@0.0.1 build

  api  ok
  app  ok
```

The summary is a result, not narration, so `--quiet` keeps it.

## setup

```sh
erno setup
```

Interactive wizard that writes `~/.erno/config.toml`. Prompts for a PostgreSQL admin connection URL (default `postgres://erno:erno@localhost:5432/postgres`), validates it can connect and create databases, then saves the file.

The admin user must have `CREATEDB` privilege:

```sql
CREATE USER erno WITH PASSWORD 'erno';
ALTER USER erno CREATEDB;
```

Run `setup` once per machine before using `doctor` or `new`.

---

## doctor

```sh
erno doctor
```

Checks everything needed to build and run Erno projects:

| Check | Required |
|-------|---------|
| Rust ≥ 1.88 | Yes |
| Node.js | Yes |
| npm | Yes |
| Angular CLI (`ng`) | Yes |
| Ionic CLI (`ionic`) | Yes |
| PostgreSQL client (`psql`) | Yes |
| PostgreSQL server running | Yes |
| `~/.erno/config.toml` | Yes |
| Admin user can `CREATE DATABASE` | Yes |
| `sea-orm-cli` | Recommended |

Exit code is `0` if all required checks pass, `1` otherwise. A warning never fails the run.

```text
==> Environment

  ok    Rust                 1.90.0
  ok    Node.js              v22.11.0
  fail  PostgreSQL server    not running
        Start it — e.g.: sudo service postgresql start
  warn  sea-orm-cli          not found
        Install with: cargo install sea-orm-cli

error: 1 required check failed
  Fix the issues above and run `erno doctor` again.
```

`erno doctor --quiet` prints only the rows that need attention, so a healthy environment produces no output at all.

---

## new

```sh
erno new <name> [--path <dir>] [--erno-path <erno-dir>] [--bundle-id <id>] [--dev|--no-dev]
```

Scaffolds a new full-stack project under `./<name>/`:

```
<name>/
├── .gitignore
├── api/                        # Rust backend (erno-based)
│   ├── Cargo.toml
│   ├── config/
│   │   ├── development.toml    # generated JWT secret, mock email, local DB
│   │   ├── production.toml
│   │   └── test.toml
│   └── src/
│       ├── main.rs
│       └── migrations/
│           └── mod.rs          # extends erno_migrations()
├── app/                        # Ionic/Angular/Capacitor product app
│   ├── package.json            # depends on erno-angular
│   ├── angular.json
│   ├── capacitor.config.ts     # Capacitor bundle ID and web dir
│   └── src/
│       └── app/
│           ├── auth/           # login, register, forgot/reset password, verify email
│           └── home/           # authenticated home page
└── www/                        # Astro static marketing site (SEO landing page)
    ├── package.json
    ├── astro.config.mjs
    └── src/pages/index.astro   # public landing → links to app /login and /register
```

Also creates the `<name>_development` and `<name>_test` PostgreSQL databases using the admin credentials from `~/.erno/config.toml`.

### Local URLs

| Surface | Dev | Production (default hosts) |
|---------|-----|----------------------------|
| Marketing (`www/`) | http://localhost:4321 | `example.com` |
| Product app (`app/`) | http://localhost:4200 | `app.example.com` |
| API (`api/`) | http://localhost:3000 | `api.example.com` |

After scaffolding, `erno new` asks whether to start `erno dev` (default yes on a TTY). `--dev` starts without asking; `--no-dev` skips. Non-interactive runs do not start servers. Landing page CTAs point at the app origin (`PUBLIC_APP_URL`, default `http://localhost:4200`).

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--path <dir>` | current directory | Parent directory for the new project |
| `--erno-path <erno-dir>` | git reference | Path to a local erno repository root or its `api/` directory (for development against an unpublished erno) |
| `--bundle-id <id>` | `com.example.<name>` | Capacitor bundle ID (reverse-DNS, no dashes) |
| `--dev` | prompt on TTY | Start `erno dev` without asking |
| `--no-dev` | prompt on TTY | Do not start `erno dev` |

### Erno dependency

Without `--erno-path`, the generated `api/Cargo.toml` and `app/package.json` reference published packages:

```toml
erno = { git = "https://github.com/tomekpiotrowski/erno" }
```
```json
"erno-angular": "^0.0.1"
```

With `--erno-path /path/to/erno`, both are pointed at local sources:

```toml
erno = { path = "/path/to/erno/api" }
```
```json
"erno-angular": "file:/path/to/erno/app/dist/erno-angular-0.0.1.tgz"
```

The CLI packs `app/dist/erno-angular` into a tarball before wiring it into the generated app, which avoids duplicate Angular runtimes from a symlinked `file:` dependency.
