use std::path::Path;
use std::process::Stdio;

use crate::global_config::GlobalConfig;
use crate::ui;

const TEMPLATE_API_DOCKERFILE: &str = include_str!("../../templates/deploy/api/Dockerfile");
const TEMPLATE_APP_DOCKERFILE: &str = include_str!("../../templates/deploy/app/Dockerfile");
const TEMPLATE_APP_NGINX_CONF: &str = include_str!("../../templates/deploy/app/docker/nginx.conf");
const TEMPLATE_APP_ENTRYPOINT: &str =
    include_str!("../../templates/deploy/app/docker/entrypoint.sh");
const TEMPLATE_WWW_DOCKERFILE: &str = include_str!("../../templates/deploy/www/Dockerfile");
const TEMPLATE_WWW_NGINX_CONF: &str = include_str!("../../templates/deploy/www/docker/nginx.conf");
const TEMPLATE_WWW_ENTRYPOINT: &str =
    include_str!("../../templates/deploy/www/docker/entrypoint.sh");
const TEMPLATE_CHART_YAML: &str = include_str!("../../templates/deploy/chart/Chart.yaml");
const TEMPLATE_VALUES_YAML: &str = include_str!("../../templates/deploy/chart/values.yaml");
const TEMPLATE_SECRETS_EXAMPLE: &str =
    include_str!("../../templates/deploy/chart/secrets.example.yaml");
const TEMPLATE_DEPLOY_TOML: &str = include_str!("../../templates/deploy/chart/deploy.toml");
const TEMPLATE_HELPERS_TPL: &str =
    include_str!("../../templates/deploy/chart/templates/_helpers.tpl");
const TEMPLATE_API_DEPLOYMENT: &str =
    include_str!("../../templates/deploy/chart/templates/api.yaml");
const TEMPLATE_API_SERVICE: &str =
    include_str!("../../templates/deploy/chart/templates/api_service.yaml");
const TEMPLATE_APP_DEPLOYMENT: &str =
    include_str!("../../templates/deploy/chart/templates/app.yaml");
const TEMPLATE_APP_SERVICE: &str =
    include_str!("../../templates/deploy/chart/templates/app_service.yaml");
const TEMPLATE_WWW_DEPLOYMENT: &str =
    include_str!("../../templates/deploy/chart/templates/www.yaml");
const TEMPLATE_WWW_SERVICE: &str =
    include_str!("../../templates/deploy/chart/templates/www_service.yaml");
const TEMPLATE_INGRESS: &str = include_str!("../../templates/deploy/chart/templates/ingress.yaml");
const TEMPLATE_LETSENCRYPT_ISSUER: &str =
    include_str!("../../templates/deploy/chart/templates/letsencrypt_issuer.yaml");
const TEMPLATE_REGISTRY_SECRET: &str =
    include_str!("../../templates/deploy/chart/templates/registry_secret.yaml");
const TEMPLATE_GITHUB_WORKFLOW: &str =
    include_str!("../../templates/deploy/github/workflows/build.yaml");
const TEMPLATE_API_PRODUCTION_TOML: &str =
    include_str!("../../templates/api/config/production.toml");
const TEMPLATE_ADMIN_DOCKERFILE: &str = include_str!("../../templates/deploy/admin/Dockerfile");
const TEMPLATE_ADMIN_NGINX: &str = include_str!("../../templates/deploy/admin/nginx.conf");
const TEMPLATE_ADMIN_ENTRYPOINT: &str = include_str!("../../templates/deploy/admin/entrypoint.sh");
const TEMPLATE_ADMIN_DEPLOYMENT: &str =
    include_str!("../../templates/deploy/chart/templates/admin.yaml");
const TEMPLATE_PROMETHEUS_DEPLOYMENT: &str =
    include_str!("../../templates/deploy/chart/templates/prometheus.yaml");

pub async fn handle_deploy_init() -> ui::Cmd {
    validate_project_root();

    let name = read_project_name();
    let github_repo = read_github_repo();
    let k8s_context = prompt_k8s_context();

    ui::section(format!("Generating deployment files for '{name}'"));
    ui::blank();

    let (admin_password, admin_password_hash) = generate_admin_password();

    let vars: &[(&str, &str)] = &[
        ("{{name}}", &name),
        ("{{github_repo}}", &github_repo),
        ("{{kubernetes_context}}", &k8s_context),
        ("{{admin_password_hash}}", &admin_password_hash),
    ];

    write_file("api/Dockerfile", render(TEMPLATE_API_DOCKERFILE, vars));
    write_file("app/Dockerfile", render(TEMPLATE_APP_DOCKERFILE, vars));
    write_file(
        "app/docker/nginx.conf",
        render(TEMPLATE_APP_NGINX_CONF, vars),
    );
    write_file(
        "app/docker/entrypoint.sh",
        render(TEMPLATE_APP_ENTRYPOINT, vars),
    );
    write_file("www/Dockerfile", render(TEMPLATE_WWW_DOCKERFILE, vars));
    write_file(
        "www/docker/nginx.conf",
        render(TEMPLATE_WWW_NGINX_CONF, vars),
    );
    write_file(
        "www/docker/entrypoint.sh",
        render(TEMPLATE_WWW_ENTRYPOINT, vars),
    );
    write_file("admin/Dockerfile", render(TEMPLATE_ADMIN_DOCKERFILE, vars));
    write_file(
        "admin/docker/nginx.conf",
        render(TEMPLATE_ADMIN_NGINX, vars),
    );
    write_file(
        "admin/docker/entrypoint.sh",
        render(TEMPLATE_ADMIN_ENTRYPOINT, vars),
    );
    write_file("chart/Chart.yaml", render(TEMPLATE_CHART_YAML, vars));
    write_file("chart/values.yaml", render(TEMPLATE_VALUES_YAML, vars));
    write_file(
        "chart/secrets.example.yaml",
        render(TEMPLATE_SECRETS_EXAMPLE, vars),
    );
    write_file("chart/deploy.toml", render(TEMPLATE_DEPLOY_TOML, vars));
    write_file(
        "chart/templates/_helpers.tpl",
        render(TEMPLATE_HELPERS_TPL, vars),
    );
    write_file(
        "chart/templates/api.yaml",
        render(TEMPLATE_API_DEPLOYMENT, vars),
    );
    write_file(
        "chart/templates/api_service.yaml",
        render(TEMPLATE_API_SERVICE, vars),
    );
    write_file(
        "chart/templates/app.yaml",
        render(TEMPLATE_APP_DEPLOYMENT, vars),
    );
    write_file(
        "chart/templates/app_service.yaml",
        render(TEMPLATE_APP_SERVICE, vars),
    );
    write_file(
        "chart/templates/www.yaml",
        render(TEMPLATE_WWW_DEPLOYMENT, vars),
    );
    write_file(
        "chart/templates/www_service.yaml",
        render(TEMPLATE_WWW_SERVICE, vars),
    );
    write_file(
        "chart/templates/admin.yaml",
        render(TEMPLATE_ADMIN_DEPLOYMENT, vars),
    );
    write_file(
        "chart/templates/prometheus.yaml",
        render(TEMPLATE_PROMETHEUS_DEPLOYMENT, vars),
    );
    write_file(
        "chart/templates/ingress.yaml",
        render(TEMPLATE_INGRESS, vars),
    );
    write_file(
        "chart/templates/letsencrypt_issuer.yaml",
        render(TEMPLATE_LETSENCRYPT_ISSUER, vars),
    );
    write_file(
        "chart/templates/registry_secret.yaml",
        render(TEMPLATE_REGISTRY_SECRET, vars),
    );
    write_file(
        ".github/workflows/build.yaml",
        render(TEMPLATE_GITHUB_WORKFLOW, vars),
    );

    ensure_production_toml(&name);

    setup_sops(&name, &github_repo).await;

    print_admin_password_once(&admin_password);
    print_next_steps(&name, &github_repo);
    Ok(())
}

/// Generate a high-entropy admin password and its Argon2 hash.
/// Only the hash is written to secrets; the plaintext is shown once.
fn generate_admin_password() -> (String, String) {
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use rand::RngCore;

    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let password = URL_SAFE_NO_PAD.encode(bytes);

    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("argon2 hash")
        .to_string();

    (password, hash)
}

fn print_admin_password_once(password: &str) {
    ui::section("Admin password");
    ui::detail("Store this in your password manager — it is shown only once.");
    ui::blank();
    ui::info(password);
    ui::blank();
    ui::detail(
        "login     https://admin.example.com\n\
         username  admin\n\
         Only the Argon2 hash was written to chart/secrets.example.yaml.\n\
         The plaintext is NOT stored in the cluster or in git.",
    );
}

pub async fn handle_deploy_install(version: &str, env: &str) -> ui::Cmd {
    validate_project_root();

    let name = read_project_name();
    let github_repo = read_github_repo();

    let context = read_deploy_context(env);

    ui::section(format!("Switching kubectl context to '{context}'"));
    run_command("kubectl", &["config", "use-context", &context]);

    let secrets_file = format!("chart/secrets.{env}.yaml");
    if !Path::new(&secrets_file).exists() {
        return Err(format!(
            "missing {secrets_file}\n\
             Copy chart/secrets.example.yaml to {secrets_file}, fill in values, and encrypt with SOPS."
        )
        .into());
    }

    let chart_ref = format!("oci://ghcr.io/{github_repo}/{name}");
    ui::section(format!("Deploying {name} {version} to {env}"));
    run_command(
        "helm",
        &[
            "secrets",
            "upgrade",
            "--install",
            &name,
            &chart_ref,
            "--version",
            version,
            "--atomic",
            "--timeout",
            "300s",
            "-f",
            &secrets_file,
        ],
    );

    ui::blank();
    ui::ok(format!("Deployed {name} {version} to {env}"));
    Ok(())
}

fn ensure_production_toml(name: &str) {
    let path = Path::new("api/config/production.toml");
    if path.exists() {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        if content.contains("CHANGE_ME") {
            ui::warn("api/config/production.toml has CHANGE_ME placeholders");
            ui::detail(
                "Helm env vars override the database URL, JWT secret, and SMTP.\n\
                 Update api_url to your actual API domain.",
            );
        } else {
            ui::ok("api/config/production.toml (existing)");
        }
        return;
    }
    let db_name = name.replace('-', "_");
    let content = render(TEMPLATE_API_PRODUCTION_TOML, &[("{{db_name}}", &db_name)]);
    write_file("api/config/production.toml", content);
}

// --- helpers ---

fn validate_project_root() {
    if !Path::new("api/Cargo.toml").exists() || !Path::new("app/package.json").exists() {
        ui::abort(
            "not an erno project root\n\
             Run this command from the directory that contains api/ and app/.",
        );
    }
    if !Path::new("www/package.json").exists() {
        ui::warn("no www/ marketing site found");
        ui::detail(
            "Newer scaffolds include www/ (Astro). Deploy still generates the www Docker/Helm files.\n\
             Add a www/ package, or remove those units from the chart if unused.",
        );
    }
}

fn read_project_name() -> String {
    let cargo_toml = std::fs::read_to_string("api/Cargo.toml")
        .unwrap_or_else(|_| ui::abort("could not read api/Cargo.toml"));

    for line in cargo_toml.lines() {
        if let Some(rest) = line.strip_prefix("name") {
            if let Some(name) = rest.trim().strip_prefix('=') {
                return name.trim().trim_matches('"').to_string();
            }
        }
    }

    ui::abort("could not parse the project name from api/Cargo.toml");
}

fn read_github_repo() -> String {
    let git_config = std::fs::read_to_string(".git/config").unwrap_or_default();
    let mut in_origin = false;
    for line in git_config.lines() {
        let trimmed = line.trim();
        if trimmed == "[remote \"origin\"]" {
            in_origin = true;
            continue;
        }
        if in_origin && trimmed.starts_with('[') {
            break;
        }
        if in_origin {
            if let Some(rest) = trimmed.strip_prefix("url") {
                if let Some(url) = rest.trim().strip_prefix('=') {
                    return extract_github_repo(url.trim());
                }
            }
        }
    }

    ui::abort(
        "could not detect the GitHub repo from .git/config remote origin\n\
         Ensure a GitHub remote is configured.",
    );
}

fn extract_github_repo(url: &str) -> String {
    // https://github.com/owner/repo.git  or  git@github.com:owner/repo.git
    let stripped = url.trim_end_matches(".git").trim_end_matches('/');

    if let Some(path) = stripped.strip_prefix("https://github.com/") {
        return path.to_string();
    }
    if let Some(path) = stripped.strip_prefix("git@github.com:") {
        return path.to_string();
    }

    ui::abort(&format!(
        "remote origin does not look like a GitHub URL: {url}"
    ));
}

fn prompt_k8s_context() -> String {
    // List available contexts from kubeconfig
    let output = std::process::Command::new("kubectl")
        .args(["config", "get-contexts", "-o", "name"])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let contexts: Vec<&str> = std::str::from_utf8(&out.stdout)
                .unwrap_or("")
                .lines()
                .collect();

            if !contexts.is_empty() {
                ui::blank();
                ui::info("Available kubectl contexts:");
                for (i, ctx) in contexts.iter().enumerate() {
                    ui::detail(format!("{}. {}", i + 1, ctx));
                }
            }
        }
    }

    ui::prompt("Kubernetes context for production", "")
}

fn read_deploy_context(env: &str) -> String {
    let content = std::fs::read_to_string("chart/deploy.toml").unwrap_or_else(|_| {
        ui::abort("missing chart/deploy.toml — run `erno deploy init` first");
    });

    let section = format!("[{env}]");
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == section {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with('[') {
            break;
        }
        if in_section {
            if let Some(rest) = trimmed.strip_prefix("kubernetes_context") {
                if let Some(val) = rest.trim().strip_prefix('=') {
                    return val.trim().trim_matches('"').to_string();
                }
            }
        }
    }

    ui::abort(&format!(
        "no kubernetes_context found for environment '{env}' in chart/deploy.toml"
    ));
}

async fn setup_sops(name: &str, github_repo: &str) {
    // Generate age keypair
    let output = std::process::Command::new("age-keygen").output();
    let Ok(out) = output else {
        ui::warn("age-keygen not found — skipping SOPS setup");
        ui::detail("Install age (https://age-encryption.org) and re-run `erno deploy init`.");
        return;
    };

    if !out.status.success() {
        ui::warn("age-keygen failed — skipping SOPS setup");
        return;
    }

    let keygen_output = String::from_utf8_lossy(&out.stdout);
    let mut public_key = String::new();
    let mut private_key_lines: Vec<&str> = Vec::new();

    for line in keygen_output.lines() {
        if let Some(pk) = line.strip_prefix("# public key: ") {
            public_key = pk.to_string();
        }
        private_key_lines.push(line);
    }

    if public_key.is_empty() {
        ui::warn("could not parse the age public key — skipping SOPS setup");
        return;
    }

    // Write .sops.yaml with the public key
    let sops_yaml = format!("creation_rules:\n  - age: \"{public_key}\"\n");
    write_file("chart/.sops.yaml", sops_yaml);

    // Try to set GitHub Actions secret via `gh` CLI
    let private_key = private_key_lines.join("\n");
    let config = GlobalConfig::load().ok();
    let github_token = config
        .as_ref()
        .and_then(|c| c.github.as_ref())
        .map(|g| g.token.as_str());

    let secret_set =
        try_set_github_secret(github_repo, "SOPS_AGE_KEY", &private_key, github_token).await;

    ui::section("SOPS");
    ui::ok("age keypair generated");
    ui::detail(format!(
        "public key:  {public_key}\n\
         written to:  chart/.sops.yaml"
    ));

    if secret_set {
        ui::ok("SOPS_AGE_KEY secret set on GitHub Actions");
    } else {
        ui::warn("could not set the GitHub Actions secret automatically");
        ui::detail("Run this to set it manually:");
        ui::detail(format!(
            "gh secret set SOPS_AGE_KEY --repo {github_repo} --body '{}'",
            private_key.replace('\n', "\\n")
        ));
    }

    ui::warn("back up your private key — it cannot be recovered");
    ui::detail(private_key_lines.join("\n"));
    let _ = name; // used in template vars
}

async fn try_set_github_secret(
    repo: &str,
    secret_name: &str,
    value: &str,
    token: Option<&str>,
) -> bool {
    // Prefer `gh` CLI if available
    if which_gh() {
        let status = std::process::Command::new("gh")
            .args([
                "secret",
                "set",
                secret_name,
                "--repo",
                repo,
                "--body",
                value,
            ])
            .status();
        if let Ok(s) = status {
            return s.success();
        }
    }

    // Fall back to GitHub API if token available
    if let Some(token) = token {
        return set_github_secret_via_api(repo, secret_name, value, token).await;
    }

    false
}

fn which_gh() -> bool {
    std::process::Command::new("which")
        .arg("gh")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn set_github_secret_via_api(
    repo: &str,
    secret_name: &str,
    value: &str,
    token: &str,
) -> bool {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

    let client = reqwest::Client::new();

    // Step 1: get repo public key
    let url = format!("https://api.github.com/repos/{repo}/actions/secrets/public-key");
    let Ok(resp) = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "erno-cli")
        .send()
        .await
    else {
        return false;
    };

    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return false;
    };
    let Some(key_id) = json["key_id"].as_str() else {
        return false;
    };
    let Some(key_b64) = json["key"].as_str() else {
        return false;
    };
    let Ok(pub_key_bytes) = BASE64.decode(key_b64) else {
        return false;
    };

    // Step 2: encrypt with NaCl sealed box (libsodium crypto_box_seal)
    // Requires the `crypto_box` crate — we approximate with `gh` CLI fallback above.
    // If we reach here without `gh`, encrypt using the `crypto_box` crate.
    // For now, skip if gh is not available; the caller already tried gh.
    let _ = (pub_key_bytes, value);

    // Step 3: PUT the encrypted secret
    let put_url = format!("https://api.github.com/repos/{repo}/actions/secrets/{secret_name}");
    let _ = (put_url, key_id);

    false
}

/// Counts what `write_file` wrote, so the summary can report a total instead of
/// thirty near-identical rows. `--verbose` still lists every path.
static FILES_WRITTEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn write_file(path: &str, content: String) {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            ui::abort(&format!(
                "could not create directory {}: {e}",
                parent.display()
            ));
        });
    }
    std::fs::write(p, content)
        .unwrap_or_else(|e| ui::abort(&format!("could not write {path}: {e}")));
    FILES_WRITTEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if ui::verbose() {
        ui::ok(path);
    }
}

fn render(template: &str, vars: &[(&str, &str)]) -> String {
    vars.iter()
        .fold(template.to_string(), |s, (k, v)| s.replace(k, v))
}

fn run_command(program: &str, args: &[&str]) {
    let status = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .unwrap_or_else(|e| ui::abort(&format!("could not run {program}: {e}")));

    if !status.success() {
        ui::abort(&format!("{program} exited with status {status}"));
    }
}

fn print_next_steps(name: &str, github_repo: &str) {
    let written = FILES_WRITTEN.load(std::sync::atomic::Ordering::Relaxed);
    ui::section("Done");
    ui::ok(format!("wrote {written} files"));
    if !ui::verbose() {
        ui::detail("Re-run with --verbose to list them.");
    }

    ui::section("Next steps");
    ui::detail(
        "1. Copy chart/secrets.example.yaml → chart/secrets.production.yaml,\n\
            fill remaining secrets (DB, JWT, SMTP), keep admin_password_hash as generated,\n\
            then encrypt:  sops --encrypt --in-place chart/secrets.production.yaml",
    );
    ui::blank();
    ui::detail(
        "2. Install prerequisites on your cluster (first time only):\n\
            helm repo add ingress-nginx https://kubernetes.github.io/ingress-nginx\n\
            helm repo add jetstack https://charts.jetstack.io\n\
            helm install ingress-nginx ingress-nginx/ingress-nginx\n\
            helm install cert-manager jetstack/cert-manager --set installCRDs=true",
    );
    ui::blank();
    ui::detail(
        "3. Push a version tag to trigger the GitHub Actions build:\n\
            git tag v0.1.0 && git push origin v0.1.0",
    );
    ui::blank();
    ui::detail("4. Deploy:\n   erno deploy install v0.1.0");
    ui::blank();
    ui::detail(
        "5. Point DNS at the ingress-nginx LoadBalancer IP:\n\
            kubectl get svc -n ingress-nginx ingress-nginx-controller\n\
            example.com          → www (marketing)\n\
            app.example.com      → app (product SPA)\n\
            api.example.com      → api",
    );
    ui::blank();
    ui::detail(format!("GitHub repo: https://github.com/{github_repo}"));
    let _ = name;
}
