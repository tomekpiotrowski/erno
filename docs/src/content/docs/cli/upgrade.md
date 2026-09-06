---
title: erno upgrade
description: Inventory and update Erno-managed packages in an existing project
---

`erno upgrade` lists every Erno-managed package in the current project, then runs the official migrators to bring them in line with **this CLI generation**.

```sh
erno upgrade           # print the plan, confirm, run
erno upgrade --dry-run # print the plan and exit
erno upgrade --yes     # skip the confirm prompt
erno upgrade --force   # allow a dirty tree, or skip the git requirement
```

Walk up from the current directory until `erno.toml` or `api/Cargo.toml` is found.

## What it inventories

| Item | Detected from | Updater |
|------|---------------|---------|
| Node.js | `node --version` | none — **prerequisite**. Angular 22 needs `^22.22.3 \|\| ^24.15.0 \|\| ^26` |
| `app` Angular | `app/package.json` `@angular/core` | `ng update` **one major at a time** |
| `app` Ionic | `app/package.json` `@ionic/angular` | `bun x @ionic/migrate` |
| `app` erno-angular | `app/package.json` | rewrite to this CLI's GitHub Release tarball URL, then `bun install` |
| `admin` Angular | `admin/package.json` | same `ng update` loop |
| `api` erno crate | `api/Cargo.toml` | rewrite the git `tag` to this CLI's version, then `cargo update -p erno`. Path deps are reported, not rewritten |

Absent trees are omitted. A project with no `admin/` does not get an admin row.

`ng update` and `@ionic/migrate` **will** touch your components (OnPush defaults, Ionic import paths, CSS). That is those tools doing their job. Erno does **not** overlay its templates on login/register/home or recopy `admin/`.

## Order

1. Refuse if Node is too old, git is missing, the directory is not a repo, or the worktree is dirty (unless `--force`). Missing git is reported as such, not as a dirty tree.
2. App Angular majors, then Ionic, then erno-angular.
3. Admin Angular majors.
4. Pin `erno` to this CLI's git tag and `cargo update -p erno`. If the crate moved, run `cargo run -- db migrate up` in `api/` afterwards — the command reminds you; it does not migrate.

Children run with `CI=true` so they cannot prompt. After the CLI has accepted the tree, `ng update` gets `--allow-dirty` and `@ionic/migrate` gets `--force`, because each earlier step in the same run writes files. Git remains the undo; the command does not commit.

## Future versions

Targets are this CLI's scaffold versions (`Angular 22`, `Ionic 9` today). When Erno moves to the next Angular major, install the new CLI and run `erno upgrade` again. The command loops `current+1 ..= target`; it does not skip majors.

See also [CLI overview](/cli/).
