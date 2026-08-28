---
title: Deploy
description: Scaffold Docker/deploy files and install releases with erno deploy
sidebar:
  order: 1
---

`erno deploy` scaffolds production packaging for a full-stack Erno project (Dockerfiles, GitHub Actions build, SOPS-encrypted secrets) and installs a version onto a Kubernetes cluster. The CLI owns the topology — it renders api / app / www / admin (or the monitoring stack) and server-side-applies them. Cluster add-ons (cert-manager, ingress-nginx) are installed the same way: `erno deploy setup` applies their upstream static YAML. Helm is not used.

Run all commands from the **project root** (the directory that contains `api/`, `app/`, and `www/`).

### Host layout

| Host (defaults in `deploy/config.toml`) | Serves |
|-----------------------------------------|--------|
| `example.com` | Marketing site (`www/` — Astro static) |
| `app.example.com` | Product SPA (`app/` — Ionic) |
| `api.example.com` | API (`api/` — Axum) |

Landing CTAs use the app host so Sign in / Get started open the product app.

## deploy init

```sh
erno deploy init
```

Interactive setup that:

1. Reads the project name from `api/Cargo.toml` and the GitHub repo from `git remote origin`
2. Prompts for a Kubernetes context (lists contexts from `kubectl` when available)
3. Generates an admin password (shown **once**) and writes only the Argon2 hash into secrets templates
4. Writes Docker, deploy config, and CI files
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
| `deploy/config.toml` | Context, hosts, replica counts, TLS |
| `deploy/secrets.example.yaml` | Secret placeholders (admin hash, DB, registry, …) |
| `deploy/extra/` | Optional extra YAML, same release labels |
| `.github/workflows/build.yaml` | Build and publish images |
| `api/config/production.toml` | Created if missing (or warned if still full of `CHANGE_ME`) |
| `deploy/.sops.yaml` | Age public key rules (when `age-keygen` is available) |

Image tags are **not** in the secrets file. `erno deploy install v1.2.3` stamps `ghcr.io/<repo>/{api,app,www,admin}:v1.2.3`.

### Admin password

The plaintext admin password is printed once at the end of `init`. Store it in a password manager. Only the Argon2 hash is written into `deploy/secrets.example.yaml` — the plaintext is **not** committed or stored in the cluster.

```sh
Open https://admin.example.com
# Username: admin
# Password: <the one-time value from init>
```

See [Admin console](/api/console/).

### Secrets and SOPS

1. Copy `deploy/secrets.example.yaml` → `deploy/secrets.production.yaml` (or another env name).
2. Fill in database URL, JWT secret, Stripe keys, SMTP, registry pull credentials.
3. Encrypt with SOPS (age), using the public key in `deploy/.sops.yaml`.
4. Keep the age private key as GitHub Actions secret `SOPS_AGE_KEY` (set automatically when `gh` / token works).

Hosts, replica counts, and the collector URL live in `deploy/config.toml` (plaintext). `monitoring_url` is both where the API reports errors and where `erno deploy install` posts the release webhook.

Install [age](https://age-encryption.org) and [sops](https://github.com/getsops/sops). kubectl 1.26+ is required for server-side apply with prune.

## deploy setup

Once per cluster, before the first `install`:

```sh
erno deploy setup
erno deploy setup                        # run again in erno-monitoring for that cluster
erno deploy setup --provider kind        # local kind instead of a cloud LoadBalancer
erno deploy setup --upgrade              # re-apply the pinned versions
```

This `kubectl apply`s the upstream release manifests:

| Add-on | What | Pinned by this CLI |
|--------|------|--------------------|
| cert-manager | CRDs + controller in `cert-manager` | `v1.21.1` |
| ingress-nginx | Ingress controller in `ingress-nginx` | `controller-v1.13.2` |

`ingress_provider` in `deploy/config.toml` selects the ingress-nginx static YAML: `cloud` (LoadBalancer, default), `kind`, or `baremetal`. `--provider` overrides the file.

If the add-on is already present, setup skips it unless `--upgrade`. `erno deploy install` refuses to proceed when they are missing.

ingress-nginx itself stopped releasing in March 2026. Erno still installs the last static manifest because the Ingress objects use class `nginx`. Switching controller is a later change; it is not a reason to keep Helm.

## deploy install

```sh
erno deploy install <version> [--env production]
```

Example:

```sh
erno deploy install v1.2.3 --env production
```

This:

1. Switches `kubectl` to the context listed under that env in `deploy/config.toml`
2. Requires `deploy/secrets.<env>.yaml` to exist
3. Decrypts secrets (SOPS, in memory), renders the topology, server-side-applies with prune, waits for Deployments (300s), and records a revision Secret
4. On wait failure: `kubectl rollout undo` for workloads that already existed; deletes objects this revision added. A first install that fails deletes everything with the instance label (never the cluster-scoped ClusterIssuer)

Typical flow: CI builds and pushes images on a git tag → you run `erno deploy install` for that tag.

The CLI generation owns the topology. Redeploying an old image tag with a newer `erno` can pick up deploy-path fixes. Run `erno deploy diff <version>` first.

```sh
erno deploy diff v1.2.3
erno deploy status
erno deploy rollback          # previous image tags, current secrets
```

## Migrating from Helm

Existing apps that still have `chart/` must convert once:

```sh
erno deploy migrate
erno deploy install <currently-live-tag>   # take ownership without moving images
```

`migrate` writes `deploy/` from `chart/deploy.toml`, `values.yaml`, and plaintext secrets. Encrypted `secrets.<env>.yaml` files are left for you to decrypt, convert, and re-encrypt. Helm templates are **not** compiled — if you customized `chart/templates`, rewrite the result as raw YAML in `deploy/extra/` (`{{release}}`, `{{version}}`, `{{namespace}}` only). `chart/` is left in place; remove it after a successful install.

## Prerequisites checklist

| Tool | Used for |
|------|----------|
| Docker | Image builds (CI or local) |
| kubectl 1.26+ | Server-side apply, prune, rollout, add-on manifests |
| sops + age | Encrypt/decrypt secrets |
| gh (optional) | Auto-set `SOPS_AGE_KEY` |
| GitHub Container Registry | Images |

## See also

- [CLI overview](/cli/) — setup, doctor, new, admin
- [Admin console](/api/console/) — configuring `[admin]` on the server
- [Boot & configuration](/api/boot/) — production config and `APP_*` overrides
- [Deploying monitoring](/monitoring/deployment/) — the separate collector release
