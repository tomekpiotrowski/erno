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
const TEMPLATE_MON_WORKFLOW: &str =
    include_str!("../../templates/deploy/github/workflows/monitoring.yaml");
const TEMPLATE_API_PRODUCTION_TOML: &str =
    include_str!("../../templates/api/config/production.toml");
const TEMPLATE_ADMIN_DOCKERFILE: &str = include_str!("../../templates/deploy/admin/Dockerfile");
const TEMPLATE_ADMIN_NGINX: &str = include_str!("../../templates/deploy/admin/nginx.conf");
const TEMPLATE_ADMIN_ENTRYPOINT: &str = include_str!("../../templates/deploy/admin/entrypoint.sh");
const TEMPLATE_ADMIN_DEPLOYMENT: &str =
    include_str!("../../templates/deploy/chart/templates/admin.yaml");

// The monitoring deployment: its own chart, its own release, its own cluster.
const TEMPLATE_MON_DOCKERFILE: &str = include_str!("../../templates/deploy/monitoring/Dockerfile");
const TEMPLATE_MON_UI_DOCKERFILE: &str =
    include_str!("../../templates/deploy/monitoring/ui/Dockerfile");
const TEMPLATE_MON_UI_NGINX: &str =
    include_str!("../../templates/deploy/monitoring/ui/docker/nginx.conf");
const TEMPLATE_MON_UI_ENTRYPOINT: &str =
    include_str!("../../templates/deploy/monitoring/ui/docker/entrypoint.sh");
const TEMPLATE_MON_CHART_YAML: &str =
    include_str!("../../templates/deploy/monitoring/chart/Chart.yaml");
const TEMPLATE_MON_VALUES_YAML: &str =
    include_str!("../../templates/deploy/monitoring/chart/values.yaml");
const TEMPLATE_MON_SECRETS_EXAMPLE: &str =
    include_str!("../../templates/deploy/monitoring/chart/secrets.example.yaml");
const TEMPLATE_MON_DEPLOY_TOML: &str =
    include_str!("../../templates/deploy/monitoring/chart/deploy.toml");
const TEMPLATE_MON_HELPERS_TPL: &str =
    include_str!("../../templates/deploy/monitoring/chart/templates/_helpers.tpl");
const TEMPLATE_MON_COLLECTOR: &str =
    include_str!("../../templates/deploy/monitoring/chart/templates/collector.yaml");
const TEMPLATE_MON_CONSOLE: &str =
    include_str!("../../templates/deploy/monitoring/chart/templates/console.yaml");
const TEMPLATE_MON_INGRESS: &str =
    include_str!("../../templates/deploy/monitoring/chart/templates/ingress.yaml");
const TEMPLATE_MON_PROMETHEUS: &str =
    include_str!("../../templates/deploy/monitoring/chart/templates/prometheus.yaml");
const TEMPLATE_MON_ISSUER: &str =
    include_str!("../../templates/deploy/monitoring/chart/templates/letsencrypt_issuer.yaml");
const TEMPLATE_MON_REGISTRY_SECRET: &str =
    include_str!("../../templates/deploy/monitoring/chart/templates/registry_secret.yaml");

/// Which deployment a `deploy` command acts on.
///
/// The two are independent releases in independent clusters — that separation
/// is the whole point of the monitoring split, so every path, name and context
/// is derived from here rather than threaded through as booleans.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, clap::ValueEnum)]
pub enum Target {
    #[default]
    App,
    Monitoring,
}

/// Everything that differs between the two deployments, resolved once.
struct TargetSpec {
    target: Target,
    /// The helm release name.
    release: String,
    /// The chart directory, relative to the project root.
    chart_dir: &'static str,
    /// The OCI chart reference.
    chart_ref: String,
}

impl TargetSpec {
    fn resolve(target: Target, name: &str, github_repo: &str) -> Self {
        let release = match target {
            Target::App => name.to_string(),
            Target::Monitoring => format!("{name}-monitoring"),
        };
        Self {
            chart_ref: format!("oci://ghcr.io/{github_repo}/{release}"),
            chart_dir: match target {
                Target::App => "chart",
                Target::Monitoring => "monitoring/deploy/chart",
            },
            release,
            target,
        }
    }

    fn secrets_file(&self, env: &str) -> String {
        format!("{}/secrets.{env}.yaml", self.chart_dir)
    }

    fn deploy_toml(&self) -> String {
        format!("{}/deploy.toml", self.chart_dir)
    }

    fn label(&self) -> &'static str {
        match self.target {
            Target::App => "application",
            Target::Monitoring => "monitoring",
        }
    }
}

pub async fn handle_deploy_init(target: Target) -> ui::Cmd {
    validate_project_root(target);

    let name = read_project_name();
    let github_repo = read_github_repo();
    let spec = TargetSpec::resolve(target, &name, &github_repo);
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
        link_ingest_token(&ingest_token);
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

    setup_sops(&name, &github_repo, &spec).await;

    print_admin_password_once(&admin_password);
    print_next_steps(&name, &github_repo);
    Ok(())
}

/// The monitoring deployment's own files. Every path is under `monitoring/`,
/// which is what keeps the two targets from colliding.
fn write_monitoring_files(vars: &[(&str, &str)]) {
    write_file(
        "monitoring/Dockerfile",
        render(TEMPLATE_MON_DOCKERFILE, vars),
    );
    write_file(
        "monitoring/ui/Dockerfile",
        render(TEMPLATE_MON_UI_DOCKERFILE, vars),
    );
    write_file(
        "monitoring/ui/docker/nginx.conf",
        render(TEMPLATE_MON_UI_NGINX, vars),
    );
    write_file(
        "monitoring/ui/docker/entrypoint.sh",
        render(TEMPLATE_MON_UI_ENTRYPOINT, vars),
    );
    for (path, template) in [
        ("Chart.yaml", TEMPLATE_MON_CHART_YAML),
        ("values.yaml", TEMPLATE_MON_VALUES_YAML),
        ("secrets.example.yaml", TEMPLATE_MON_SECRETS_EXAMPLE),
        ("deploy.toml", TEMPLATE_MON_DEPLOY_TOML),
        ("templates/_helpers.tpl", TEMPLATE_MON_HELPERS_TPL),
        ("templates/collector.yaml", TEMPLATE_MON_COLLECTOR),
        ("templates/console.yaml", TEMPLATE_MON_CONSOLE),
        ("templates/ingress.yaml", TEMPLATE_MON_INGRESS),
        ("templates/prometheus.yaml", TEMPLATE_MON_PROMETHEUS),
        ("templates/letsencrypt_issuer.yaml", TEMPLATE_MON_ISSUER),
        (
            "templates/registry_secret.yaml",
            TEMPLATE_MON_REGISTRY_SECRET,
        ),
    ] {
        write_file(
            &format!("monitoring/deploy/chart/{path}"),
            render(template, vars),
        );
    }
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

/// Write the generated ingest token into the *application* chart too.
///
/// This is the one value that has to match across two charts in two clusters,
/// and a mismatch fails silently — the API's reports are rejected with a 401
/// and nothing says so. Filling both sides here is the only place that can
/// close the loop automatically.
fn link_ingest_token(token: &str) {
    let path = Path::new("chart/secrets.example.yaml");
    if !path.exists() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    // Only fill an empty field: never overwrite a token already in use.
    let Some(updated) = fill_empty_ingest_token(&content, token) else {
        ui::info("chart/secrets.example.yaml already has an ingest_token — left as is");
        ui::detail(
            "It must equal collector.server_token in\n             monitoring/deploy/chart/secrets.<env>.yaml.",
        );
        return;
    };
    if std::fs::write(path, updated).is_ok() {
        ui::ok("chart/secrets.example.yaml (ingest_token linked)");
    }
}

/// Replace an empty `ingest_token: ""` with `token`. `None` when there is no
/// such key, or it already has a value.
fn fill_empty_ingest_token(content: &str, token: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut filled = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if !filled && (trimmed == "ingest_token: \"\"" || trimmed == "ingest_token: ''") {
            let indent = &line[..line.len() - line.trim_start().len()];
            out.push(format!("{indent}ingest_token: \"{token}\""));
            filled = true;
        } else {
            out.push(line.to_string());
        }
    }
    filled.then(|| out.join("\n") + "\n")
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
         Only the Argon2 hash was written to chart/secrets.example.yaml.\n\
         The plaintext is NOT stored in the cluster or in git.",
    );
}

pub async fn handle_deploy_install(version: &str, env: &str, target: Target) -> ui::Cmd {
    validate_project_root(target);

    let name = read_project_name();
    let github_repo = read_github_repo();
    let spec = TargetSpec::resolve(target, &name, &github_repo);

    let context = read_deploy_context(&spec, env);

    ui::section(
        ui::icon::CLOUD,
        format!("Switching kubectl context to '{context}'"),
    );
    run_command("kubectl", &["config", "use-context", &context]);

    let secrets_file = spec.secrets_file(env);
    if !Path::new(&secrets_file).exists() {
        return Err(format!(
            "missing {secrets_file}\n\
             Copy {}/secrets.example.yaml to {secrets_file}, fill in values, and encrypt with SOPS.",
            spec.chart_dir
        )
        .into());
    }

    ui::section(
        ui::icon::DEPLOY,
        format!("Deploying {} {version} to {env}", spec.release),
    );
    run_command(
        "helm",
        &[
            "secrets",
            "upgrade",
            "--install",
            &spec.release,
            &spec.chart_ref,
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
    ui::ok(format!("Deployed {} {version} to {env}", spec.release));

    // A version only becomes a *release* when something starts serving it,
    // which is here — not when CI publishes the chart. Recording it at publish
    // time would put deploy markers on charts for versions never installed.
    //
    // Deliberately after helm succeeds: `--atomic` means a failed upgrade has
    // already rolled back, so recording it would be a lie.
    record_release(&spec, version, env).await;
    Ok(())
}

/// Tell the collector that a version is now live.
///
/// Never fatal. A deployment that actually succeeded must not be reported as
/// failed because the monitoring deployment was unreachable.
async fn record_release(spec: &TargetSpec, version: &str, env: &str) {
    let Some(collector_url) = read_deploy_value(spec, env, "monitoring_url") else {
        return;
    };
    let Ok(token) = std::env::var("ERNO_INGEST_TOKEN") else {
        ui::info("skipping the release webhook — ERNO_INGEST_TOKEN is not set");
        return;
    };

    let url = format!(
        "{}/api/collector/releases",
        collector_url.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "version": version,
        "environment": env,
        "commit_sha": commit_sha_for(version),
        "source": "cli",
    });

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    match client
        .post(&url)
        .header("X-Erno-Ingest-Key", token)
        .json(&body)
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            ui::ok(format!("recorded release {version} with the collector"));
        }
        Ok(res) => ui::warn(format!(
            "the collector rejected the release webhook ({})",
            res.status()
        )),
        Err(e) => ui::warn(format!("could not reach the collector: {e}")),
    }
}

/// The commit this version was built from — but only when the local HEAD really
/// is that tag. Otherwise the working copy has moved on and a SHA taken from it
/// would describe something other than what was deployed, which is worse than
/// sending nothing.
fn commit_sha_for(version: &str) -> Option<String> {
    let described = std::process::Command::new("git")
        .args(["describe", "--exact-match", "--tags"])
        .output()
        .ok()?;
    if !described.status.success() {
        return None;
    }
    let tag = String::from_utf8(described.stdout).ok()?;
    if tag.trim() != version
        && tag.trim().trim_start_matches('v') != version.trim_start_matches('v')
    {
        return None;
    }
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    head.status
        .success()
        .then(|| String::from_utf8_lossy(&head.stdout).trim().to_string())
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

fn validate_project_root(target: Target) {
    if !Path::new("api/Cargo.toml").exists() || !Path::new("app/package.json").exists() {
        ui::abort(
            "not an erno project root\n\
             Run this command from the directory that contains api/ and app/.",
        );
    }
    if target == Target::Monitoring {
        // api/Cargo.toml is still where the project name comes from, so the
        // check above stands; this adds what the monitoring target needs.
        if !Path::new("monitoring/Cargo.toml").exists() {
            ui::abort(
                "this project has no monitoring/\n\
                 It predates monitoring scaffolding. Copy monitoring/ from an erno\n\
                 checkout, or re-scaffold with `erno new --erno-path <path-to-erno>`.",
            );
        }
        if !Path::new("monitoring/ui/package.json").exists() {
            ui::abort("monitoring/ui is missing — the operator console cannot be built");
        }
        return;
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
            if let Ok(existing) = std::fs::read_to_string("chart/deploy.toml") {
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

fn read_deploy_context(spec: &TargetSpec, env: &str) -> String {
    let path = spec.deploy_toml();
    let content = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        ui::abort(&format!(
            "missing {path} — run `erno deploy init{}` first",
            match spec.target {
                Target::App => "",
                Target::Monitoring => " --target monitoring",
            }
        ));
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

async fn setup_sops(name: &str, github_repo: &str, spec: &TargetSpec) {
    let sops_path = format!("{}/.sops.yaml", spec.chart_dir);

    // The age key is per *repository*, not per target: one SOPS_AGE_KEY secret
    // decrypts both charts. So if one already exists, reuse its recipient.
    // Generating a fresh key here would silently make every existing
    // secrets.<env>.yaml undecryptable — including the other target's.
    if let Some(public_key) = existing_age_recipient(&["chart/.sops.yaml", &sops_path]) {
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

/// A plain (non-secret) key out of a target's deploy.toml.
///
/// The collector URL lives here rather than in secrets.<env>.yaml because that
/// file is SOPS-encrypted and unreadable when deploying the *application*
/// target — and a hostname is not a secret.
fn read_deploy_value(spec: &TargetSpec, env: &str, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(spec.deploy_toml()).ok()?;
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
            if let Some(rest) = trimmed.strip_prefix(key) {
                if let Some(val) = rest.trim().strip_prefix('=') {
                    let val = val.trim().trim_matches('"');
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
    }
    None
}

fn print_monitoring_next_steps(name: &str) {
    ui::next_steps(
        "Next steps",
        &[
            "cp monitoring/deploy/chart/secrets.example.yaml \\\n     monitoring/deploy/chart/secrets.production.yaml".to_string(),
            "sops -e -i monitoring/deploy/chart/secrets.production.yaml".to_string(),
            "git tag v0.1.0 && git push --tags".to_string(),
            format!("erno deploy install v0.1.0 --target monitoring   # release {name}-monitoring"),
        ],
    );
    ui::blank();
    ui::info("Two values must match across the two charts:");
    ui::detail(
        "collector.server_token  ==  api.error_reporting.ingest_token\n\
         api.metrics_auth_token  ==  the application's api.metrics_auth_token\n\
         A mismatch fails silently — reports are rejected and nothing says so.",
    );
    ui::blank();
    ui::info("Point monitoring.example.com at the monitoring cluster's LoadBalancer,");
    ui::detail("and add the app/admin origins to monitoring/config/production.toml [cors].");
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
            "cp chart/secrets.example.yaml chart/secrets.production.yaml".to_string(),
            "sops --encrypt --in-place chart/secrets.production.yaml".to_string(),
        ],
    );
    ui::detail("Fill in DB, JWT, and SMTP; keep admin_password_hash as generated.");
    ui::blank();
    ui::next_steps(
        "2. Install cluster prerequisites (first time only)",
        &[
            "helm repo add ingress-nginx https://kubernetes.github.io/ingress-nginx".to_string(),
            "helm repo add jetstack https://charts.jetstack.io".to_string(),
            "helm install ingress-nginx ingress-nginx/ingress-nginx".to_string(),
            "helm install cert-manager jetstack/cert-manager --set installCRDs=true".to_string(),
        ],
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
    fn each_target_gets_its_own_release_chart_and_paths() {
        let app = TargetSpec::resolve(Target::App, "acme", "acme/acme");
        assert_eq!(app.release, "acme");
        assert_eq!(app.chart_dir, "chart");
        assert_eq!(app.chart_ref, "oci://ghcr.io/acme/acme/acme");
        assert_eq!(
            app.secrets_file("production"),
            "chart/secrets.production.yaml"
        );
        assert_eq!(app.deploy_toml(), "chart/deploy.toml");

        let mon = TargetSpec::resolve(Target::Monitoring, "acme", "acme/acme");
        assert_eq!(mon.release, "acme-monitoring");
        assert_eq!(mon.chart_dir, "monitoring/deploy/chart");
        assert_eq!(mon.chart_ref, "oci://ghcr.io/acme/acme/acme-monitoring");
        assert_eq!(
            mon.secrets_file("production"),
            "monitoring/deploy/chart/secrets.production.yaml"
        );
        assert_eq!(mon.deploy_toml(), "monitoring/deploy/chart/deploy.toml");
    }

    #[test]
    fn the_two_targets_never_share_a_path_or_a_release() {
        let app = TargetSpec::resolve(Target::App, "acme", "acme/acme");
        let mon = TargetSpec::resolve(Target::Monitoring, "acme", "acme/acme");
        assert_ne!(app.release, mon.release);
        assert_ne!(app.chart_ref, mon.chart_ref);
        assert_ne!(app.chart_dir, mon.chart_dir);
        // Every monitoring path is under monitoring/, which is what keeps
        // `deploy init --target monitoring` from overwriting the app's chart.
        assert!(mon.chart_dir.starts_with("monitoring/"));
        assert!(mon.secrets_file("staging").starts_with("monitoring/"));
    }

    #[test]
    fn an_empty_ingest_token_is_filled_and_an_existing_one_is_left_alone() {
        let empty = "api:\n  error_reporting:\n    ingest_token: \"\"\n";
        let filled = fill_empty_ingest_token(empty, "s3cret").expect("should fill");
        assert!(filled.contains("ingest_token: \"s3cret\""));
        // Indentation is preserved, or the YAML stops being valid.
        assert!(filled.contains("    ingest_token:"));

        // Never clobber a token already in use: the other cluster is using it.
        let existing = "api:\n  error_reporting:\n    ingest_token: \"live\"\n";
        assert!(fill_empty_ingest_token(existing, "s3cret").is_none());
        assert!(fill_empty_ingest_token("api: {}\n", "s3cret").is_none());
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
