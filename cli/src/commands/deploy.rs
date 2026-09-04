use std::path::Path;

use crate::deploy::{self, Layout};
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
const TEMPLATE_SECRETS_EXAMPLE: &str = include_str!("../../templates/deploy/secrets.example.yaml");
const TEMPLATE_DEPLOY_CONFIG: &str = include_str!("../../templates/deploy/config.toml");
const TEMPLATE_GITHUB_WORKFLOW: &str =
    include_str!("../../templates/deploy/github/workflows/build.yaml");
const TEMPLATE_MON_WORKFLOW: &str =
    include_str!("../../templates/deploy/github/workflows/monitoring.yaml");
const TEMPLATE_API_PRODUCTION_TOML: &str =
    include_str!("../../templates/api/config/production.toml");
const TEMPLATE_ADMIN_DOCKERFILE: &str = include_str!("../../templates/deploy/admin/Dockerfile");
const TEMPLATE_ADMIN_NGINX: &str = include_str!("../../templates/deploy/admin/nginx.conf");
const TEMPLATE_ADMIN_ENTRYPOINT: &str = include_str!("../../templates/deploy/admin/entrypoint.sh");

// The monitoring deployment: its own release, its own cluster.
const TEMPLATE_MON_DOCKERFILE: &str = include_str!("../../templates/deploy/monitoring/Dockerfile");
const TEMPLATE_MON_UI_DOCKERFILE: &str =
    include_str!("../../templates/deploy/monitoring/console/Dockerfile");
const TEMPLATE_MON_UI_NGINX: &str =
    include_str!("../../templates/deploy/monitoring/console/docker/nginx.conf");
const TEMPLATE_MON_UI_ENTRYPOINT: &str =
    include_str!("../../templates/deploy/monitoring/console/docker/entrypoint.sh");
const TEMPLATE_MON_SECRETS_EXAMPLE: &str =
    include_str!("../../templates/deploy/monitoring/secrets.example.yaml");
const TEMPLATE_MON_CONFIG: &str = include_str!("../../templates/deploy/monitoring/config.toml");

pub use crate::deploy::Target;

/// Everything that differs between the two deployments, resolved once.
struct TargetSpec {
    target: Target,
    layout: Layout,
}

impl TargetSpec {
    fn resolve(target: Target) -> Self {
        Self {
            layout: Layout::for_target(target),
            target,
        }
    }

    fn label(&self) -> &'static str {
        self.target.label()
    }
}

pub async fn handle_deploy_init(target: Target) -> ui::Cmd {
    deploy::validate_project_root(target);

    let name = deploy::read_project_name();
    let github_repo = deploy::read_github_repo();
    let spec = TargetSpec::resolve(target);
    let k8s_context = prompt_k8s_context(target);

    ui::section(
        ui::icon::DEPLOY,
        format!("Generating {} deployment files for '{name}'", spec.label()),
    );
    ui::blank();

    // A separate console with separate credentials, so the monitoring target
    // generates its own rather than reusing the application's.
    let (admin_password, admin_password_hash) = generate_admin_password();
    // The trusted server-to-server token. Generated here so the operator never
    // has to invent one, and so both halves of the link can be written at once.
    let ingest_token = generate_token();

    let vars: &[(&str, &str)] = &[
        ("{{name}}", &name),
        ("{{github_repo}}", &github_repo),
        ("{{kubernetes_context}}", &k8s_context),
        ("{{monitoring_kubernetes_context}}", &k8s_context),
        ("{{admin_password_hash}}", &admin_password_hash),
        ("{{ingest_token}}", &ingest_token),
    ];

    if target == Target::Monitoring {
        write_monitoring_files(vars);
        setup_sops(&name, &github_repo, &spec).await;
        warn_if_erno_is_a_path_dependency();
        print_admin_password_once(&admin_password);
        print_monitoring_next_steps(&name);
        return Ok(());
    }

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
    write_file("deploy/config.toml", render(TEMPLATE_DEPLOY_CONFIG, vars));
    write_file(
        "deploy/secrets.example.yaml",
        render(TEMPLATE_SECRETS_EXAMPLE, vars),
    );
    write_file(
        ".github/workflows/build.yaml",
        render(TEMPLATE_GITHUB_WORKFLOW, vars),
    );

    ensure_production_toml(&name);

    setup_sops(&name, &github_repo, &spec).await;

    print_admin_password_once(&admin_password);
    print_next_steps(&name, &github_repo);
    Ok(())
}

/// The collector's deployment files, written into its own repository.
///
/// It is laid out as an ordinary Erno application, so the collector's image
/// builds from `api/` and the operator console's from `app/`.
fn write_monitoring_files(vars: &[(&str, &str)]) {
    write_file("api/Dockerfile", render(TEMPLATE_MON_DOCKERFILE, vars));
    write_file("app/Dockerfile", render(TEMPLATE_MON_UI_DOCKERFILE, vars));
    write_file("app/docker/nginx.conf", render(TEMPLATE_MON_UI_NGINX, vars));
    write_file(
        "app/docker/entrypoint.sh",
        render(TEMPLATE_MON_UI_ENTRYPOINT, vars),
    );
    write_file("deploy/config.toml", render(TEMPLATE_MON_CONFIG, vars));
    write_file(
        "deploy/secrets.example.yaml",
        render(TEMPLATE_MON_SECRETS_EXAMPLE, vars),
    );
    // Its own workflow, never the application's: two independent deployables
    // with independent release cadences, and merging YAML with a `str::replace`
    // renderer is not a thing.
    write_file(
        ".github/workflows/monitoring.yaml",
        render(TEMPLATE_MON_WORKFLOW, vars),
    );
}

/// A URL-safe random token, for the shared ingest secret.
fn generate_token() -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Warn when `erno` is a path dependency, which no image build can satisfy.
///
/// A Docker build only sees its context. A path like `../../erno/api` resolves
/// on the machine that scaffolded this and nowhere else, so the collector image
/// fails to build in CI with a confusing cargo error. Saying so at init time is
/// the difference between a five-second fix and a red pipeline.
fn warn_if_erno_is_a_path_dependency() {
    let Ok(manifest) = std::fs::read_to_string("api/Cargo.toml") else {
        return;
    };
    if !erno_dep_is_path(&manifest) {
        return;
    }
    ui::warn("api/Cargo.toml depends on erno by path — the image will not build");
    ui::detail(
        "A Docker build sees only its context, so a path outside this repository\n\
         cannot resolve. Point erno (and erno-error-reporting-types) at a git\n\
         revision before tagging a release.",
    );
}

/// Whether the manifest's `erno` dependency is a local path.
fn erno_dep_is_path(manifest: &str) -> bool {
    manifest
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("erno ") || l.starts_with("erno="))
        .any(|l| l.contains("path"))
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
    ui::section(ui::icon::KEY, "Admin password");
    ui::detail("Store this in your password manager — it is shown only once.");
    ui::blank();
    ui::info(password);
    ui::blank();
    ui::detail(
        "login     https://admin.example.com\n\
         username  admin\n\
         Only the Argon2 hash was written to deploy/secrets.example.yaml.\n\
         The plaintext is NOT stored in the cluster or in git.",
    );
}

pub async fn handle_deploy_install(version: &str, env: &str, target: Target) -> ui::Cmd {
    deploy::handle_install(version, env, target).await
}

fn ensure_production_toml(name: &str) {
    let path = Path::new("api/config/production.toml");
    if path.exists() {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        if content.contains("CHANGE_ME") {
            ui::warn("api/config/production.toml has CHANGE_ME placeholders");
            ui::detail(
                "Deploy env vars override the database URL, JWT secret, and SMTP.\n\
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

fn prompt_k8s_context(target: Target) -> String {
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

    match target {
        Target::App => ui::prompt("Kubernetes context for production", ""),
        Target::Monitoring => {
            let context = ui::prompt(
                "Kubernetes context for monitoring (a different cluster from the application)",
                "",
            );
            // Sharing a cluster defeats the entire point: the monitoring stack
            // would go down with the outage it exists to report.
            if let Ok(existing) = std::fs::read_to_string("deploy/config.toml") {
                if existing.contains(&format!("\"{context}\"")) {
                    ui::warn("that is the same context the application deploys to");
                    ui::detail(
                        "A monitoring stack sharing a failure domain with what it monitors\n\
                         goes down with it. Use separate infrastructure if you can.",
                    );
                }
            }
            context
        }
    }
}

async fn setup_sops(name: &str, github_repo: &str, spec: &TargetSpec) {
    let sops_path = spec.layout.sops_path().display().to_string();

    // The age key is per *repository*, not per target: one SOPS_AGE_KEY secret
    // decrypts both secret files. So if one already exists, reuse its recipient.
    // Generating a fresh key here would silently make every existing
    // secrets.<env>.yaml undecryptable — including the other target's.
    if let Some(public_key) =
        existing_age_recipient(&["deploy/.sops.yaml", "chart/.sops.yaml", &sops_path])
    {
        write_file(
            &sops_path,
            format!("creation_rules:\n  - age: \"{public_key}\"\n"),
        );
        ui::section(ui::icon::KEY, "SOPS");
        ui::ok("reusing the existing age key");
        ui::detail(format!(
            "public key:  {public_key}\n\
             written to:  {sops_path}\n\
             A new key would make every existing secrets.<env>.yaml undecryptable."
        ));
        return;
    }

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
    write_file(&sops_path, sops_yaml);

    // Try to set GitHub Actions secret via `gh` CLI
    let private_key = private_key_lines.join("\n");
    let config = GlobalConfig::load().ok();
    let github_token = config
        .as_ref()
        .and_then(|c| c.github.as_ref())
        .map(|g| g.token.as_str());

    let secret_set =
        try_set_github_secret(github_repo, "SOPS_AGE_KEY", &private_key, github_token).await;

    ui::section(ui::icon::KEY, "SOPS");
    ui::ok("age keypair generated");
    ui::detail(format!(
        "public key:  {public_key}\n\
         written to:  {sops_path}"
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

fn print_monitoring_next_steps(name: &str) {
    ui::next_steps(
        "Next steps",
        &[
            "cp deploy/secrets.example.yaml deploy/secrets.production.yaml".to_string(),
            "sops -e -i deploy/secrets.production.yaml".to_string(),
            "erno deploy setup".to_string(),
            "git tag v0.1.0 && git push --tags".to_string(),
            format!("erno deploy install v0.1.0   # release {name}"),
        ],
    );
    ui::blank();
    ui::info("Two values must match across the two deployments:");
    ui::detail(
        "api.ingest_token        ==  this app's project server token on the collector\n\
         api.metrics_auth_token  ==  the application's api.metrics_auth_token\n\
         A mismatch fails silently — reports are rejected and nothing says so.",
    );
    ui::blank();
    ui::info("Ingest tokens live on the collector's project rows, not in its config.");
    ui::detail(
        "The collector mints one per project. error_reporting.ingest_token in the\n\
         monitoring secrets seeds the first project on an empty database.",
    );
    ui::blank();
    ui::info("Point monitoring.example.com at the monitoring cluster's LoadBalancer,");
    ui::detail("and add the app/admin origins to api/config/production.toml [cors].");
}

/// The age recipient already configured for this repository, if any.
///
/// Pure enough to test: it reads files, but the parsing is what matters and
/// `age_recipient_from` below is what carries the logic.
fn existing_age_recipient(paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|p| {
        std::fs::read_to_string(p)
            .ok()
            .as_deref()
            .and_then(age_recipient_from)
    })
}

/// Pull the `age:` recipient out of a `.sops.yaml`.
fn age_recipient_from(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches('-').trim();
        if let Some(rest) = trimmed.strip_prefix("age:") {
            let key = rest.trim().trim_matches('"').trim_matches('\'');
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
    }
    None
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

fn print_next_steps(name: &str, github_repo: &str) {
    let written = FILES_WRITTEN.load(std::sync::atomic::Ordering::Relaxed);
    ui::section(ui::icon::DONE, "Done");
    ui::ok(format!("wrote {written} files"));
    if !ui::verbose() {
        ui::detail("Re-run with --verbose to list them.");
    }

    ui::section(ui::icon::NOTE, "Next steps");
    ui::blank();
    ui::next_steps(
        "1. Encrypt your production secrets",
        &[
            "cp deploy/secrets.example.yaml deploy/secrets.production.yaml".to_string(),
            "sops --encrypt --in-place deploy/secrets.production.yaml".to_string(),
        ],
    );
    ui::detail("Fill in DB, JWT, and SMTP; keep admin_password_hash as generated.");
    ui::blank();
    ui::next_steps(
        "2. Install cluster add-ons (first time only)",
        &["erno deploy setup".to_string()],
    );
    ui::blank();
    ui::next_steps(
        "3. Push a version tag to trigger the GitHub Actions build",
        &["git tag v0.1.0 && git push origin v0.1.0".to_string()],
    );
    ui::blank();
    ui::next_steps("4. Deploy", &["erno deploy install v0.1.0".to_string()]);
    ui::blank();
    ui::next_steps(
        "5. Point DNS at the ingress-nginx LoadBalancer IP",
        &["kubectl get svc -n ingress-nginx ingress-nginx-controller".to_string()],
    );
    ui::detail(
        "example.com          → www (marketing)\n\
         app.example.com      → app (product SPA)\n\
         api.example.com      → api",
    );
    ui::blank();
    ui::detail(format!("GitHub repo: https://github.com/{github_repo}"));
    let _ = name;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_build_workflow_healthchecks_postgres_without_app_env() {
        let yaml = render(TEMPLATE_GITHUB_WORKFLOW, &[("{{name}}", "acme")]);
        assert!(yaml.contains("pg_isready -U acme"), "{yaml}");
        assert!(
            !yaml.contains("APP_ENV"),
            "setup_test reads config/test.toml, not APP_ENV:\n{yaml}"
        );
        assert!(
            !yaml.contains("DATABASE_URL"),
            "setup_test reads config/test.toml, not DATABASE_URL:\n{yaml}"
        );
    }

    #[test]
    fn init_seeds_the_collector_but_leaves_the_application_token_empty() {
        let token = "a-generated-token";
        let vars: &[(&str, &str)] = &[
            ("{{admin_password_hash}}", "hash"),
            ("{{ingest_token}}", token),
        ];

        // The collector hashes this into its seeded `monitoring` project on an
        // empty database, which is how it can accept anything at all on day one.
        let rendered = render(TEMPLATE_MON_SECRETS_EXAMPLE, vars);
        assert!(!rendered.contains("{{ingest_token}}"));
        let parsed: serde_yaml::Value =
            serde_yaml::from_str(&rendered).expect("rendered monitoring secrets parse");
        assert_eq!(
            parsed["error_reporting"]["ingest_token"].as_str(),
            Some(token)
        );

        // An application's token is not minted here. It is a project's server
        // token, which the collector mints per project, so the scaffold ships an
        // empty slot for the operator to paste into.
        assert!(
            TEMPLATE_SECRETS_EXAMPLE.contains("ingest_token: \"\""),
            "the app scaffold must ship an empty ingest_token"
        );
    }

    #[test]
    fn a_path_dependency_on_erno_is_flagged_before_it_reaches_ci() {
        // A Docker build sees only its context, so these never resolve.
        assert!(erno_dep_is_path(r#"erno = { path = "../../erno/api" }"#));
        assert!(erno_dep_is_path("erno = { path = \"../api\" }"));
        // The forms an image can actually build from.
        assert!(!erno_dep_is_path(
            r#"erno = { git = "https://github.com/acme/erno", rev = "abc123" }"#
        ));
        assert!(!erno_dep_is_path(r#"erno = "0.1""#));
        // A different crate that merely starts with the same letters.
        assert!(!erno_dep_is_path(
            r#"erno-error-reporting-types = { path = "../x" }"#
        ));
    }

    #[test]
    fn both_targets_deploy_from_the_same_directory() {
        // The collector has its own repository, so the two charts are never in
        // one tree and no longer need separate paths to avoid clobbering each
        // other. Each repo has one `deploy/`.
        for target in [Target::App, Target::Monitoring] {
            let spec = TargetSpec::resolve(target);
            assert_eq!(spec.layout.dir, "deploy");
            assert_eq!(
                spec.layout.secrets_path("production").to_str().unwrap(),
                "deploy/secrets.production.yaml"
            );
            assert_eq!(
                spec.layout.config_path().to_str().unwrap(),
                "deploy/config.toml"
            );
        }
    }

    #[test]
    fn the_age_recipient_is_read_back_rather_than_regenerated() {
        // Re-running `deploy init` must reuse the key, or every existing
        // secrets.<env>.yaml in the repo becomes undecryptable.
        let sops = "creation_rules:\n  - age: \"age1abc123\"\n";
        assert_eq!(age_recipient_from(sops).as_deref(), Some("age1abc123"));
        assert_eq!(age_recipient_from("creation_rules: []\n"), None);
        assert_eq!(age_recipient_from("  - age: \"\"\n"), None);
    }

    #[test]
    fn a_generated_token_is_long_and_url_safe() {
        let token = generate_token();
        assert!(token.len() >= 40, "{token}");
        assert!(token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(token, generate_token());
    }
}
