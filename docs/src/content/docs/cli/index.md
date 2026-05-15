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
erno new <name> [--path <dir>] [--erno-path <erno-dir>] [--bundle-id <id>]
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
└── app/                        # Ionic/Angular/Capacitor frontend
    ├── package.json            # depends on erno-angular
    ├── angular.json
    ├── capacitor.config.ts     # Capacitor bundle ID and web dir
    └── src/
        ├── global.scss         # Ionic global styles
        ├── theme/
        │   └── variables.scss  # Ionic CSS custom properties
        └── app/
            ├── app.module.ts   # IonicModule + ErnoModule.forRoot() wired up
            ├── app-routing.module.ts
            ├── app.component.ts
            ├── app.component.html
            ├── auth/           # login, register, forgot/reset password, verify email
            └── home/           # authenticated home page
```

Also creates the `<name>_development` and `<name>_test` PostgreSQL databases using the admin credentials from `~/.erno/config.toml`.

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--path <dir>` | current directory | Parent directory for the new project |
| `--erno-path <erno-dir>` | git reference | Path to a local erno repository root or its `api/` directory (for development against an unpublished erno) |
| `--bundle-id <id>` | `com.example.<name>` | Capacitor bundle ID (reverse-DNS, no dashes) |

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
