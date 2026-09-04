# Erno CLI

The `erno` binary. Commands run from `cli/`.

```sh
cargo build
cargo run -- <command>
cargo test
```

Does not depend on the `api/` crate.

**All terminal writes go through `src/ui.rs`.** No `println!` / `eprintln!` elsewhere — `tests/output_goes_through_ui.rs` fails the build if one is added. stdout is program output; stderr is narration. Prefer returning `ui::Failure` over exiting so Drop guards still run.

`erno new` templates live under `cli/templates/` and are pulled in with `include_str!` from `new.rs` (CI is `cli/templates/github/workflows/ci.yml`). `build` / `lint` / `test` share the sequential runner in `packages.rs`. `erno dev` children are non-interactive (piped stdio); pass flags instead of prompting.

To add a command: `src/commands/<name>.rs` with `handle_<name>`, declare it in `commands/mod.rs`, wire it in `main.rs`.

Narrative docs: `docs/src/content/docs/cli/`.

`deploy/extra/` is how a tree adds workloads the CLI does not know (interpolates `{{release}}` / `{{version}}` / `{{namespace}}` / `{{env.NAME}}`). Do not grow first-class store types in the renderer.
