use std::io::{self, Write};
use std::path::Path;
use std::process::Stdio;

use crate::global_config::GlobalConfig;

const TEMPLATE_API_DOCKERFILE: &str =
    include_str!("../../templates/deploy/api/Dockerfile");
const TEMPLATE_APP_DOCKERFILE: &str =
    include_str!("../../templates/deploy/app/Dockerfile");
const TEMPLATE_APP_NGINX_CONF: &str =
    include_str!("../../templates/deploy/app/docker/nginx.conf");
const TEMPLATE_APP_ENTRYPOINT: &str =
    include_str!("../../templates/deploy/app/docker/entrypoint.sh");
const TEMPLATE_CHART_YAML: &str =
    include_str!("../../templates/deploy/chart/Chart.yaml");
const TEMPLATE_VALUES_YAML: &str =
    include_str!("../../templates/deploy/chart/values.yaml");
const TEMPLATE_SECRETS_EXAMPLE: &str =
    include_str!("../../templates/deploy/chart/secrets.example.yaml");
const TEMPLATE_DEPLOY_TOML: &str =
    include_str!("../../templates/deploy/chart/deploy.toml");
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
const TEMPLATE_INGRESS: &str =
    include_str!("../../templates/deploy/chart/templates/ingress.yaml");
const TEMPLATE_LETSENCRYPT_ISSUER: &str =
    include_str!("../../templates/deploy/chart/templates/letsencrypt_issuer.yaml");
const TEMPLATE_REGISTRY_SECRET: &str =
    include_str!("../../templates/deploy/chart/templates/registry_secret.yaml");
const TEMPLATE_GITHUB_WORKFLOW: &str =
    include_str!("../../templates/deploy/github/workflows/build.yaml");
const TEMPLATE_API_PRODUCTION_TOML: &str =
    include_str!("../../templates/api/config/production.toml");

pub async fn handle_deploy_init() {
    validate_project_root();

    let name = read_project_name();
    let github_repo = read_github_repo();
    let k8s_context = prompt_k8s_context();

    println!("\n📦  Generating deployment files for '{name}'...\n");

    let vars: &[(&str, &str)] = &[
        ("{{name}}", &name),
        ("{{github_repo}}", &github_repo),
        ("{{kubernetes_context}}", &k8s_context),
    ];

    write_file("api/Dockerfile", render(TEMPLATE_API_DOCKERFILE, vars));
    write_file("app/Dockerfile", render(TEMPLATE_APP_DOCKERFILE, vars));
    write_file("app/docker/nginx.conf", render(TEMPLATE_APP_NGINX_CONF, vars));
    write_file("app/docker/entrypoint.sh", render(TEMPLATE_APP_ENTRYPOINT, vars));
    write_file("chart/Chart.yaml", render(TEMPLATE_CHART_YAML, vars));
    write_file("chart/values.yaml", render(TEMPLATE_VALUES_YAML, vars));
    write_file("chart/secrets.example.yaml", render(TEMPLATE_SECRETS_EXAMPLE, vars));
    write_file("chart/deploy.toml", render(TEMPLATE_DEPLOY_TOML, vars));
    write_file("chart/templates/_helpers.tpl", render(TEMPLATE_HELPERS_TPL, vars));
    write_file("chart/templates/api.yaml", render(TEMPLATE_API_DEPLOYMENT, vars));
    write_file("chart/templates/api_service.yaml", render(TEMPLATE_API_SERVICE, vars));
    write_file("chart/templates/app.yaml", render(TEMPLATE_APP_DEPLOYMENT, vars));
    write_file("chart/templates/app_service.yaml", render(TEMPLATE_APP_SERVICE, vars));
    write_file("chart/templates/ingress.yaml", render(TEMPLATE_INGRESS, vars));
    write_file("chart/templates/letsencrypt_issuer.yaml", render(TEMPLATE_LETSENCRYPT_ISSUER, vars));
    write_file("chart/templates/registry_secret.yaml", render(TEMPLATE_REGISTRY_SECRET, vars));
    write_file(".github/workflows/build.yaml", render(TEMPLATE_GITHUB_WORKFLOW, vars));

    ensure_production_toml(&name);

    setup_sops(&name, &github_repo).await;

    print_next_steps(&name, &github_repo);
}

pub async fn handle_deploy_install(version: &str, env: &str) {
    validate_project_root();

    let name = read_project_name();
    let github_repo = read_github_repo();

    let context = read_deploy_context(env);

    println!("🔀  Switching kubectl context to '{context}'...");
    run_command("kubectl", &["config", "use-context", &context]);

    let secrets_file = format!("chart/secrets.{env}.yaml");
    if !Path::new(&secrets_file).exists() {
        eprintln!("❌  Missing {secrets_file}");
        eprintln!("    Copy chart/secrets.example.yaml to {secrets_file}, fill in values, and encrypt with SOPS.");
        std::process::exit(1);
    }

    let chart_ref = format!("oci://ghcr.io/{github_repo}/{name}");
    println!("🚀  Deploying {name} {version} to {env}...");
    run_command(
        "helm",
        &[
            "secrets", "upgrade", "--install", &name,
            &chart_ref,
            "--version", version,
            "--atomic",
            "--timeout", "300s",
            "-f", &secrets_file,
        ],
    );

    println!("\n✅  Deployed {name} {version} to {env}.");
}

fn ensure_production_toml(name: &str) {
    let path = Path::new("api/config/production.toml");
    if path.exists() {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        if content.contains("CHANGE_ME") {
            println!("  ⚠️  api/config/production.toml — has CHANGE_ME placeholders");
            println!("     Helm env vars will override database URL, JWT secret, and SMTP.");
            println!("     Remember to update api_url to your actual API domain.");
        } else {
            println!("  ✓ api/config/production.toml (existing)");
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
        eprintln!("❌  Not an erno project root.");
        eprintln!("    Run this command from the directory that contains api/ and app/.");
        std::process::exit(1);
    }
}

fn read_project_name() -> String {
    let cargo_toml = std::fs::read_to_string("api/Cargo.toml")
        .unwrap_or_else(|_| { eprintln!("❌  Could not read api/Cargo.toml"); std::process::exit(1) });

    for line in cargo_toml.lines() {
        if let Some(rest) = line.strip_prefix("name") {
            if let Some(name) = rest.trim().strip_prefix('=') {
                return name.trim().trim_matches('"').to_string();
            }
        }
    }

    eprintln!("❌  Could not parse project name from api/Cargo.toml");
    std::process::exit(1);
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

    eprintln!("⚠️   Could not detect GitHub repo from .git/config remote origin.");
    eprintln!("    Please ensure a GitHub remote is configured.");
    std::process::exit(1);
}

fn extract_github_repo(url: &str) -> String {
    // https://github.com/owner/repo.git  or  git@github.com:owner/repo.git
    let stripped = url
        .trim_end_matches(".git")
        .trim_end_matches('/');

    if let Some(path) = stripped.strip_prefix("https://github.com/") {
        return path.to_string();
    }
    if let Some(path) = stripped.strip_prefix("git@github.com:") {
        return path.to_string();
    }

    eprintln!("❌  Remote origin does not look like a GitHub URL: {url}");
    std::process::exit(1);
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
                println!("\nAvailable kubectl contexts:");
                for (i, ctx) in contexts.iter().enumerate() {
                    println!("  {}. {}", i + 1, ctx);
                }
            }
        }
    }

    prompt("\nKubernetes context for production", "")
}

fn read_deploy_context(env: &str) -> String {
    let content = std::fs::read_to_string("chart/deploy.toml").unwrap_or_else(|_| {
        eprintln!("❌  Missing chart/deploy.toml — run `erno deploy init` first.");
        std::process::exit(1);
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

    eprintln!("❌  No kubernetes_context found for environment '{env}' in chart/deploy.toml");
    std::process::exit(1);
}

async fn setup_sops(name: &str, github_repo: &str) {
    // Generate age keypair
    let output = std::process::Command::new("age-keygen").output();
    let Ok(out) = output else {
        println!("⚠️   age-keygen not found — skipping SOPS setup.");
        println!("    Install age (https://age-encryption.org) and re-run `erno deploy init`.");
        return;
    };

    if !out.status.success() {
        println!("⚠️   age-keygen failed — skipping SOPS setup.");
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
        println!("⚠️   Could not parse age public key — skipping SOPS setup.");
        return;
    }

    // Write .sops.yaml with the public key
    let sops_yaml = format!("creation_rules:\n  - age: \"{public_key}\"\n");
    write_file("chart/.sops.yaml", sops_yaml);

    // Try to set GitHub Actions secret via `gh` CLI
    let private_key = private_key_lines.join("\n");
    let config = GlobalConfig::load().ok();
    let github_token = config.as_ref().and_then(|c| c.github.as_ref()).map(|g| g.token.as_str());

    let secret_set = try_set_github_secret(github_repo, "SOPS_AGE_KEY", &private_key, github_token).await;

    println!("\n🔑  Age keypair generated.");
    println!("    Public key:  {public_key}");
    println!("    Written to:  chart/.sops.yaml");

    if secret_set {
        println!("    SOPS_AGE_KEY secret set on GitHub Actions ✅");
    } else {
        println!("\n    ⚠️   Could not set GitHub Actions secret automatically.");
        println!("    Run this command to set it manually:");
        println!("      gh secret set SOPS_AGE_KEY --repo {github_repo} --body '{}'", private_key.replace('\n', "\\n"));
    }

    println!("\n    ⚠️   Back up your private key — it cannot be recovered:");
    println!("{}", private_key_lines.join("\n"));
    let _ = name; // used in template vars
}

async fn try_set_github_secret(repo: &str, secret_name: &str, value: &str, token: Option<&str>) -> bool {
    // Prefer `gh` CLI if available
    if which_gh() {
        let status = std::process::Command::new("gh")
            .args(["secret", "set", secret_name, "--repo", repo, "--body", value])
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

async fn set_github_secret_via_api(repo: &str, secret_name: &str, value: &str, token: &str) -> bool {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let client = reqwest::Client::new();

    // Step 1: get repo public key
    let url = format!("https://api.github.com/repos/{repo}/actions/secrets/public-key");
    let Ok(resp) = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "erno-cli")
        .send()
        .await else { return false; };

    let Ok(json) = resp.json::<serde_json::Value>().await else { return false; };
    let Some(key_id) = json["key_id"].as_str() else { return false; };
    let Some(key_b64) = json["key"].as_str() else { return false; };
    let Ok(pub_key_bytes) = BASE64.decode(key_b64) else { return false; };

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

fn write_file(path: &str, content: String) {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("❌  Could not create directory {}: {e}", parent.display());
            std::process::exit(1);
        });
    }
    std::fs::write(p, content).unwrap_or_else(|e| {
        eprintln!("❌  Could not write {path}: {e}");
        std::process::exit(1);
    });
    println!("  ✓ {path}");
}

fn render(template: &str, vars: &[(&str, &str)]) -> String {
    vars.iter().fold(template.to_string(), |s, (k, v)| s.replace(k, v))
}

fn prompt(label: &str, default: &str) -> String {
    print!("{label}: ");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).expect("failed to read stdin");
    let trimmed = input.trim();
    if trimmed.is_empty() { default.to_string() } else { trimmed.to_string() }
}

fn run_command(program: &str, args: &[&str]) {
    let status = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .unwrap_or_else(|e| {
            eprintln!("❌  Failed to run {program}: {e}");
            std::process::exit(1);
        });

    if !status.success() {
        eprintln!("❌  {program} exited with status {status}");
        std::process::exit(1);
    }
}

fn print_next_steps(name: &str, github_repo: &str) {
    println!("\n✅  Deployment scaffold complete.\n");
    println!("Next steps:\n");
    println!("  1. Fill in chart/secrets.production.yaml (copy from chart/secrets.example.yaml)");
    println!("     then encrypt it:  sops --encrypt --in-place chart/secrets.production.yaml");
    println!();
    println!("  2. Install prerequisites on your cluster (first time only):");
    println!("     helm repo add ingress-nginx https://kubernetes.github.io/ingress-nginx");
    println!("     helm repo add jetstack https://charts.jetstack.io");
    println!("     helm install ingress-nginx ingress-nginx/ingress-nginx");
    println!("     helm install cert-manager jetstack/cert-manager --set installCRDs=true");
    println!();
    println!("  3. Push a version tag to trigger the GitHub Actions build:");
    println!("     git tag v0.1.0 && git push origin v0.1.0");
    println!();
    println!("  4. Deploy:");
    println!("     erno deploy install v0.1.0");
    println!();
    println!("  5. Point your DNS CNAME records to the ingress-nginx LoadBalancer IP:");
    println!("     kubectl get svc -n ingress-nginx ingress-nginx-controller");
    println!();
    println!("  GitHub repo: https://github.com/{github_repo}");
    let _ = name;
}
