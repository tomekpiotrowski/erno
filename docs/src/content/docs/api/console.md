---
title: Admin console
description: HTTP admin API and erno admin TUI
sidebar:
  order: 9
---

> **Source**: `api/src/admin/` (server), `cli/src/admin/` (TUI client)

Erno ships an operator admin surface for users, subscriptions, and jobs:

1. **HTTP API** under `/admin/api/*` (Basic auth, Argon2 password hash)
2. **`erno admin`** — a Ratatui TUI that talks to that API

This works against **local and production** APIs without opening a database tunnel: the server process owns domain logic (including `UserDataDeleter` and job enqueue).

## Enable the admin API

Configure an Argon2 password hash:

```toml
[admin]
username = "admin"
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$..."
```

Or via environment (production / Helm):

```bash
APP__ADMIN__PASSWORD_HASH='...'
APP__ADMIN__USERNAME=admin   # optional, default admin
```

If `password_hash` is missing or empty, admin routes are **not mounted**.

### Development

Scaffolded and library `config/development.toml` use password **`admin`**. Hash is precomputed so local `erno admin` works out of the box.

### Production

`erno deploy init` generates a random password, prints it **once**, and writes **only the hash** into `chart/secrets.example.yaml`. Helm injects `APP__ADMIN__PASSWORD_HASH`. Never put the plaintext password in cluster secrets or git.

## Launch the TUI

```bash
# Local (API running via erno dev / cargo run)
# Uses password "admin" automatically for localhost — no prompt
erno admin

# Production
erno admin --url https://api.example.com
# prompts for password (from your password manager)

# Non-interactive
ERNO_ADMIN_PASSWORD='...' erno admin --url https://api.example.com
erno admin --url https://api.example.com --password-env MY_SECRET_VAR
```

| Flag | Description |
|------|-------------|
| `--url` | API base URL (default: `api_url` from `api/config/development.toml` or `http://localhost:3000`) |
| `--user` | Basic-auth username (default `admin`) |
| `--password` | Password (prefer prompt / env) |
| `--password-env` | Read password from this env var |

## API surface

All routes require `Authorization: Basic …` and live under `/admin/api`.

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/dashboard` | User + job summary |
| `GET` | `/users?q=` | Search users |
| `GET` | `/users/{id}` | User detail + subscription |
| `POST` | `/users/{id}/activate` | Mark email verified |
| `DELETE` | `/users/{id}` | Purge user (same path as account deletion) |
| `POST` | `/users/{id}/gift` | Gift subscription `{ "plan", "duration_days" }` |
| `GET` | `/jobs?status=&type=` | Job stats + list |
| `POST` | `/jobs/{id}/retry` | Re-queue a job |
| `GET` | `/plans` | Plan names from Stripe config |
| `GET` | `/stats?days=` | Business-stat sparkline history (`stat_snapshot`) |

Admin routes are rate-limited under the `admin` action.

## TUI screens

### Dashboard

Live summary of users by subscription type and job queue health.  
`r` refresh · `u` users · `j` jobs · `s` stats · `q` quit.

### Business stats

Sparklines from daily `stat_snapshot` rows (see [Business stats](./business-stats)).  
`w` cycle window (7/30/90 days) · `r` refresh · `Esc` dashboard.

### Users

Search by email, open detail with Enter.

| Key | Action |
|-----|--------|
| `g` | Gift a subscription |
| `a` | Activate (verify email) |
| `x` | Delete (type email to confirm) |
| `Esc` | Back |

### Jobs

Stats by type (top) and job list (bottom). `Tab` switches panels.

| Key | Action |
|-----|--------|
| `f` | Cycle status filter |
| `t` | Filter by selected type |
| `r` | Retry failed job / refresh stats |
| `Esc` | Dashboard |

## Security notes

- Production stores **hash only**; rotate by generating a new password, hashing, updating SOPS secrets, redeploying.
- Prefer TLS (ingress) for production admin.
- Admin is intended for the CLI/operators, not the Angular SPA (no special CORS for admin).
