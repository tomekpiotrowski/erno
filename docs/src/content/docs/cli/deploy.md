---
title: Deploy
description: Scaffold Docker/Helm deploy files and install releases with erno deploy
sidebar:
  order: 1
---

`erno deploy` scaffolds production packaging for a full-stack Erno project (Dockerfiles, Helm chart, GitHub Actions build, SOPS-encrypted secrets) and installs a chart version onto a Kubernetes cluster.

Run all commands from the **project root** (the directory that contains `api/`, `app/`, and `www/`).

### Host layout

| Host (defaults in secrets) | Serves |
|----------------------------|--------|
| `example.com` | Marketing site (`www/` — Astro static) |
| `app.example.com` | Product SPA (`app/` — Ionic) |
| `api.example.com` | API (`api/` — Axum) |

Landing CTAs use `www.app_url` (e.g. `https://app.example.com`) so Sign in / Get started open the product app.

## deploy init

```sh
erno deploy init
```

Interactive setup that:

1. Reads the project name from `api/Cargo.toml` and the GitHub repo from `git remote origin`
2. Prompts for a Kubernetes context (lists contexts from `kubectl` when available)
3. Generates an admin password (shown **once**) and writes only the Argon2 hash into secrets templates
4. Writes Docker, Helm, and CI files
5. Optionally generates an age keypair for SOPS and sets `SOPS_AGE_KEY` on GitHub Actions

### Files generated

| Path | Purpose |
|------|---------|
| `api/Dockerfile` | Multi-stage build for the Rust API |
| `app/Dockerfile` | Product SPA build + nginx |
| `app/docker/nginx.conf` | SPA static serving |
| `app/docker/entrypoint.sh` | Injects runtime `API_URL` into the SPA |
| `www/Dockerfile` | Marketing site build + nginx |
| `www/docker/nginx.conf` | Static site serving |
| `www/docker/entrypoint.sh` | Injects runtime `APP_URL` into landing links |
| `chart/Chart.yaml` | Helm chart metadata |
| `chart/values.yaml` | Default values |
| `chart/secrets.example.yaml` | Secret placeholders (admin hash, DB, hosts, …) |
| `chart/deploy.toml` | Maps environment name → kubectl context |
| `chart/templates/*` | API/app/www Deployments, Services, Ingress, cert-manager issuer, registry secret |
| `.github/workflows/build.yaml` | Build and publish images/chart |
| `api/config/production.toml` | Created if missing (or warned if still full of `CHANGE_ME`) |
| `chart/.sops.yaml` | Age public key rules (when `age-keygen` is available) |

### Admin password

The plaintext admin password is printed once at the end of `init`. Store it in a password manager. Only the Argon2 hash is written into `chart/secrets.example.yaml` — the plaintext is **not** committed or stored in the cluster.

```sh
Open https://admin.example.com
# Username: admin
# Password: <the one-time value from init>
```

See [Admin console](/api/console/).

### Secrets and SOPS

1. Copy `chart/secrets.example.yaml` → `chart/secrets.production.yaml` (or another env name).
2. Fill in database URL, JWT secret, Stripe keys, SMTP, etc.
3. Encrypt with SOPS (age), using the public key in `chart/.sops.yaml`.
4. Keep the age private key as GitHub Actions secret `SOPS_AGE_KEY` (set automatically when `gh` / token works).

Install [age](https://age-encryption.org) and [helm-secrets](https://github.com/jkroepke/helm-secrets) for the encrypt/install path.

## deploy install

```sh
erno deploy install <version> [--env production]
```

Example:

```sh
erno deploy install v1.2.3 --env production
```

This:

1. Switches `kubectl` to the context listed under that env in `chart/deploy.toml`
2. Requires `chart/secrets.<env>.yaml` to exist
3. Runs `helm secrets upgrade --install` against the OCI chart  
   `oci://ghcr.io/<github_repo>/<project_name>` at the given version

Typical flow: CI builds and pushes the chart/images → you run `erno deploy install` for a tag.

## Prerequisites checklist

| Tool | Used for |
|------|----------|
| Docker | Image builds (CI or local) |
| kubectl | Cluster context |
| Helm + helm-secrets | Install with encrypted values |
| age / age-keygen | SOPS key material |
| gh (optional) | Auto-set `SOPS_AGE_KEY` |
| GitHub Container Registry | OCI chart + images |

## See also

- [CLI overview](/cli/) — setup, doctor, new, admin
- [Admin console](/api/console/) — configuring `[admin]` on the server
- [Boot & configuration](/api/boot/) — production config and `APP_*` overrides
