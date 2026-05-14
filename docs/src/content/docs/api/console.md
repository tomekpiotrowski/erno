---
title: Admin TUI
description: Interactive terminal UI for administration
sidebar:
  order: 9
---

> **Source**: `api/src/admin/` (feature: `tui`)

Erno includes an interactive terminal UI (powered by [Ratatui](https://ratatui.rs/)) for administering a running application. Launch it with:

```bash
cargo run -- admin
```

## Screens

### Dashboard

The default screen shows a live summary of users by subscription type (Stripe, gift, trial, none) and the current job queue (pending, running, failed counts).

Press `r` to refresh, `u` to switch to Users, `j` to switch to Jobs, `q` to quit.

### Users

Browse and search users by email. Use arrow keys to navigate, type to filter, and `Enter` to open a user's detail screen.

From the detail screen:

| Key | Action |
|-----|--------|
| `g` | Gift a subscription (choose plan and duration in days) |
| `a` | Activate the user (mark email as verified) |
| `x` | Delete the user (requires typing the email to confirm) |
| `Esc` | Back |

### Jobs

Two panels: a stats table grouped by job type (top) and a scrollable job list (bottom). Switch panels with `Tab`.

| Key | Action |
|-----|--------|
| `f` | Cycle status filter (all → failed → pending → running) |
| `t` | Filter by the job type selected in the top panel |
| `r` | Retry the selected failed job (bottom panel) / refresh (top panel) |
| `Esc` | Back to Dashboard |
