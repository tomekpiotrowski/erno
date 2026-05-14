# Erno

Rust/Axum SaaS infrastructure library — batteries-included auth, jobs, billing, sync, storage, and an offline-first sync engine.

## Monorepo layout

| Directory | What it is |
|-----------|------------|
| `api/`    | Main Rust library crate — see `api/AGENTS.md` for development instructions |
| `app/`    | Angular library (`erno-angular`) for Ionic web + mobile — see `app/AGENTS.md` |
| `docs/`   | Astro documentation site |

## API (Rust)

`api/` is the Rust library crate — auth, jobs, billing, sync, storage, and more. See `api/AGENTS.md` for build instructions, module reference, and architecture notes.

## App (Angular)

`app/` contains `erno-angular` — an Angular 20 library that Ionic apps consume for web and mobile. See `app/AGENTS.md` for build instructions, service reference, and architecture notes.

## Docs (Astro)

```sh
cd docs
npm run dev        # dev server
npm run build      # production build
```
