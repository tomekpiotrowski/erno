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
| `erno new <name>` | Scaffolds a full-stack Erno project (Rust API + Ionic Angular standalone app + Astro www) |
| `erno upgrade` | Inventories Erno-managed packages and runs official migrators (`ng update`, `@ionic/migrate`) toward this CLI generation |
| `erno dev` | Starts api + app + www dev servers, readiness banner, `--ios`/`--android` live reload (`--target <id>` picks the device) |
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
| `src/main.rs` | CLI entry point — clap command definitions, global flags, and dispatch |
| `src/ui.rs` | **Every** terminal write — styling, rows, sections, prefixes, prompts, errors, and the pinned `dev` region |
| `src/global_config.rs` | `~/.erno/config.toml` read/write via the `config` crate |
| `src/commands/setup.rs` | Interactive config writer; validates admin connection before saving |
| `src/commands/doctor.rs` | Environment checks — each returns a `CheckResult` (a `ui::Row` plus whether it is required) |
| `src/commands/new.rs` | Project scaffolding — inline templates, directory creation, database creation |
| `src/commands/dev/` | `erno dev` — process multiplexer, readiness banner, quiet logs |
| `src/commands/packages.rs` | `erno.toml` parsing, package selection, and the sequential phase runner shared by build/lint/test |
| `src/commands/build.rs` | `erno build` — runs the `build` phase |
| `src/commands/lint.rs` | `erno lint` — runs the `lint` phase, with `--fix` |
| `src/commands/test.rs` | `erno test` — the `test` phase, plus test-database setup and e2e orchestration |
| `src/commands/upgrade/` | `erno upgrade` — inventory scanners + official migrators; targets are this CLI generation |

## Output style

**All terminal output goes through `src/ui.rs`.** No other file calls `println!`/`eprintln!`, and `tests/output_goes_through_ui.rs` fails the build if one does. If you need to print something, use an existing helper or add one there.

### The visual language

```text
==> Section header              column 0, `==>` blue+bold, title bold
  ok    a row                   column 2, marker green
  warn  another row             marker yellow
  fail  a third row             marker red
        a continuation line     column 8, dim
error: something went wrong     column 0, `error:` red+bold
[api] a forwarded child line    `[api]` in the service colour, text verbatim
```

- **Markers are ASCII words, never emoji.** `✅`/`❌` are East-Asian Wide (2 columns) while `⚠️`/`ℹ️` are 1 column plus a variation selector and render at 1 *or* 2 depending on terminal and font — no single pad count aligns them everywhere. Words are one column per character on every terminal, and they survive `--no-color` and piping.
- **Colour is decoration only.** Nothing is communicated by colour alone.
- **One indent rule**: rows at 2, their continuations at 8 (`ui::CONTINUATION`), section headers and fatal errors at 0. Nothing else has an indent. Pass multi-line text to `ui::detail` rather than hand-indenting with `\n      `.
- **Column widths are computed** with `ui::column_width`, never hardcoded.
- The exception is text the CLI only *forwards*: `dev/log.rs` matches on emoji emitted by the `api/` crate's own startup and migration output. That is not ours to restyle.

### stdout vs stderr

**stdout is the program's output; stderr is the program's narration.** Section headers, rows, banners, prompts, warnings, errors, and summaries all go to stderr. Forwarded subprocess output goes to whichever stream it came from. So `erno doctor > out.txt` is empty by design, and `erno build 2>/dev/null` leaves only what the child tools printed to stdout.

### Global flags

`--no-color`, `--quiet`/`-q`, and `--verbose`/`-v` are `global = true`, so they work on either side of the subcommand. `ui::init` resolves them once in `main` into module state; deep call sites read `ui::color()` / `ui::quiet()` / `ui::verbose()` rather than threading a context struct.

Colour resolution order: `--no-color`, then `NO_COLOR`, then `CLICOLOR_FORCE`, then "is stderr a TTY that understands ANSI".

`--quiet` suppresses section headers, `ok`/`info` rows, and details. It never suppresses warnings, errors, forwarded child output, or a command's final result (the per-package `ok`/`fail` summary).

### Errors and exit codes

Every `handle_*` returns `ui::Cmd` (`Result<(), ui::Failure>`). `main` renders `Failure::Message` through `ui::fatal` and returns `ExitCode::FAILURE`; `Failure::Silent` means the command already reported the details (e.g. `run_phase` printed the summary). Internal helpers keep returning `Result<T, String>` — `From<String>` means a bare `?` works.

Prefer returning `Failure` over exiting: it unwinds, so guards like `DevLock` still run their `Drop`. `ui::abort` exists for the scaffolding helpers in `new`/`deploy`, which are called from deep inside `write`-style chains where there is nothing to clean up.

### Testing output

Everything visible is a pure `render_*(on: bool, …) -> String` with a thin printing wrapper. Test the pure half — that keeps tests free of global state and order-independent. `ui::init` is never called under `cargo test`, so colour defaults to off.

The pinned region follows the same rule: `render_frame`, `truncate_display`, `fit_region`, `region_fits`, and `strip_cursor_control` are pure and unit-tested; only `pin`/`repin`/`frame` touch the terminal. What is left — the `ioctl`, real cursor motion, and cross-task interleaving — has no pty harness in the tree and is verified by hand:

```
erno dev                     # region pins, states flip in place, logs scroll above
  resize the window mid-run  # next redraw re-fits: no wrap, no stray rows
Ctrl+C                       # final banner left once in the scrollback
erno dev | cat               # stdout piped: no flicker, region correct on stderr
erno dev 2>/dev/null         # no escapes anywhere
erno dev --no-color / -q / -v / ERNO_STICKY=0 / TERM=dumb   # fallback in all five
```

### The pinned region

`erno dev` pins its status banner to the last rows of the terminal and redraws it in place as services become ready, with logs scrolling above it. This is the only cursor control in the CLI, it lives entirely in `ui.rs`, and it is stderr-only.

- **`ui::pin(lines) -> Option<Pinned>`** starts it; `ui::repin(lines)` replaces the content; dropping the `Pinned` guard erases the region and leaves its final contents in the scrollback. The guard is modelled on `DevLock` — it is what makes an early `?` safe. `ui::fatal` also clears the region, which covers the paths that `exit` without unwinding.
- **`None` means fall back**, and the fallback is the behaviour the CLI has always had: the banner printed once, then one `ok`/`info` row per state change. `dev/banner.rs` keeps both renderers, and `spawn_readiness_watcher` takes a `sticky` flag rather than reading a global. Transition rows are suppressed while the region is live — the region already shows every state, and printing both is the duplication this replaced.
- **The predicate**: a unix terminal on stderr (`is_terminal` is checked separately from `color()`, because `CLICOLOR_FORCE` can make colour true for a file), ANSI enabled, not `--quiet`, not `--verbose` (that is the raw multiplex), `ERNO_STICKY != 0`, and `ui::region_fits` — the whole region fitting as it is, with rows to spare above it.
- **The invariant**: the region occupies the last `drawn` rows and the cursor sits at column 0 below it, so every frame emits exactly `body + region` newlines. Terminal size comes from `TIOCGWINSZ` on each redraw (`SIGWINCH` is not handled); `ui::fit_region` and `ui::truncate_display` guarantee no region line wraps, because a wrapped region line would break the cursor-up count. Forwarded child text is passed through `ui::strip_cursor_control` while a region is live, so a tool drawing its own progress cannot dislodge it.
- **One output mutex** serialises every write on both streams, which is what makes erase → write → redraw atomic against `dev`'s ~19 printing tasks. Nothing holding that lock may call a `ui` function that takes it. Multi-line renders go out through `ui::emit_block` as one frame, so another task's `[api] …` can no longer land mid-banner.

### Deliberate non-goals

The escape vocabulary is exactly two sequences — cursor-up and erase-to-end-of-display. No alternate screen, no raw mode, no cursor hiding, no spinners, no `indicatif`. A SIGKILL therefore leaves the terminal in a completely normal state, and every renderer stays a pure function.

## Architecture notes

- **No dependency on `api/`**: the CLI does not depend on the `erno` library crate. Admin uses `reqwest` + `ratatui` as an HTTP client. Keeping it decoupled avoids version skew and circular concerns.
- **One manifest, three commands**: `build`, `lint`, and `test` differ only in which phase they run and whether `--fix` applies. The shared engine lives in `packages.rs`; the command modules are thin. `test.rs` is the only one that needs more, because the e2e package is orchestrated rather than shelled out — it passes a callback that `run_phase` gives first refusal on each package.
- **Templates are inline strings**: `new.rs` holds all scaffold templates as Rust string constants/functions. `{{name}}` is substituted via `.replace()` — no template engine dependency.
- **`erno upgrade` is an orchestrator**: scanners list Erno-managed packages; official tools (`ng update` one major at a time, `@ionic/migrate`) do the rewriting. Targets (`TARGET_ANGULAR_MAJOR`, `TARGET_IONIC_MAJOR`) are this CLI generation. Children run with `CI=true`.
- **`erno_migrations()` helper**: scaffolded apps call `erno::database::migrations::erno_migrations()` in their `Migrator` to include all built-in framework migrations (users, jobs, sync, billing, storage) before their own.
- **Database creation**: `erno new` connects with the admin URL from `~/.erno/config.toml` and issues `CREATE DATABASE` for `<name>_development` and `<name>_test`.
- **`dev` children can never be interactive**: every child is spawned into its own process group with piped stdio, so reading the terminal raises `SIGTTIN` and stops it — a hang with an unanswerable question buried in a pipe. Anything a child would prompt for must be decided by the CLI beforehand and passed as a flag (`npx --yes`, `ionic --no-interactive --target`). As a backstop, `dev/process.rs` flushes an unterminated line once the stream goes quiet and `dev/log.rs` forwards prompt-shaped lines even in quiet mode.

## Adding a new command

1. Create `src/commands/<command>.rs` with a `pub async fn handle_<command>(...)` function.
2. Add `pub mod <command>;` to `src/commands/mod.rs`.
3. Add a variant to the `Commands` enum in `src/main.rs` and dispatch it in `main()`.
