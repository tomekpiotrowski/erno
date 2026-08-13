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
| [`erno dev`](#dev) | Start the API, app, and www dev servers |
| [`erno admin`](#admin) | Operator TUI against the running API (`/admin/api`) |
| [`erno deploy`](/cli/deploy/) | Scaffold Docker/Helm files and install releases |

---

## dev

```sh
erno dev
erno dev --verbose
erno dev --api
erno dev --app --www
erno dev --no-www
```

Starts the project’s dev servers (`api/` + `app/`, plus `www/` when present). Walks up from the current directory looking for `api/Cargo.toml`, so you can run it from `api/`, `app/`, or any subdirectory. Child tools are told to keep color and cargo’s progress bar even though their stdout is piped.

By default only errors and ready events are printed; the full multiplex is written to `.erno/dev.log`. Pass `--verbose` (or `-v`) to stream every child line, prefixed by service (`[api]`, `[app]`, `[www]`).

`--api`, `--app`, and `--www` start only the services you name (combine them). `--no-www` skips the marketing site when you want the default API + app pair. `--api` does not require an `app/` directory.

`erno dev` prints a status banner with each service URL and probes them until they respond:

| Surface | Probe | Default URL (overridden by project config) |
|---------|-------|-------------|
| API | `GET /readiness` (`/liveness` while migrating) | `[server].port` / `api_url` in `api/config/development.toml` |
| Product app | HTTP | `app_url` or `angular.json` serve port (4200) |
| Marketing | HTTP | `--port` in `www/package.json` `dev` script (4321) |

A second `erno dev` in the same project is rejected via `.erno/dev.lock` (stale locks from a crashed session are replaced). When the API is running the banner also lists `erno admin` (password `admin`), `/dev/emails`, and `/dev/jobs`. Newly captured mock emails are printed as `[mail] subject → to`. The banner reprints whenever a service changes state (`starting` → `ready`). If one process exits, it is restarted (with backoff) without taking the others down. The API is rebuilt automatically when `api/` source files change (no `cargo-watch` needed). Ctrl+C sends SIGTERM, then SIGKILL after two seconds.

Before spawning anything, `erno dev` checks that PostgreSQL is running (when the API is selected) and that each selected service’s port is free. If a port is held by a leftover `cargo`/`node`/`erno` process, it offers to kill it.

---

## admin

```sh
erno admin
erno admin --url https://api.example.com
```

Interactive TUI for users, gifts, and jobs. Talks to the API over HTTP with Basic auth — see [Admin console](/api/console).

Against localhost, the password defaults to `admin` (no prompt). Production password is generated once by `erno deploy init` (hash only stored in the cluster).

---

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

Exit code is `0` if all required checks pass, `1` otherwise.

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
