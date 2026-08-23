use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;
use rand::Rng;

use crate::global_config::GlobalConfig;
use crate::ui;

// ── Embedded templates ────────────────────────────────────────────────────────

const GITIGNORE: &str = include_str!("../../templates/.gitignore");
const API_CARGO_TOML: &str = include_str!("../../templates/api/Cargo.toml");
const API_LIB_RS: &str = include_str!("../../templates/api/src/lib.rs");
const API_MAIN_RS: &str = include_str!("../../templates/api/src/main.rs");
const API_MIGRATIONS_MOD_RS: &str = include_str!("../../templates/api/src/migrations/mod.rs");
const API_TESTS_COMMON: &str = include_str!("../../templates/api/tests/common/mod.rs");
const API_TESTS_HEALTH: &str = include_str!("../../templates/api/tests/health.rs");
const API_DEVELOPMENT_TOML: &str = include_str!("../../templates/api/config/development.toml");
const API_PRODUCTION_TOML: &str = include_str!("../../templates/api/config/production.toml");
const API_TEST_TOML: &str = include_str!("../../templates/api/config/test.toml");
const APP_MAIN_TS: &str = include_str!("../../templates/app/src/main.ts");
const APP_COMPONENT_TS: &str = include_str!("../../templates/app/src/app/app.component.ts");
const APP_ENVIRONMENT_TS: &str =
    include_str!("../../templates/app/src/environments/environment.ts");
const APP_ENVIRONMENT_PROD_TS: &str =
    include_str!("../../templates/app/src/environments/environment.prod.ts");
const APP_COMPONENT_HTML: &str = include_str!("../../templates/app/app.component.html");
const APP_ROUTES_TS: &str = include_str!("../../templates/app/src/app/app.routes.ts");
const AUTH_GUARD_TS: &str = include_str!("../../templates/app/src/app/auth/auth.guard.ts");
const LOGIN_COMPONENT_TS: &str =
    include_str!("../../templates/app/src/app/auth/login/login.component.ts");
const LOGIN_COMPONENT_HTML: &str =
    include_str!("../../templates/app/src/app/auth/login/login.component.html");
const REGISTER_COMPONENT_TS: &str =
    include_str!("../../templates/app/src/app/auth/register/register.component.ts");
const REGISTER_COMPONENT_HTML: &str =
    include_str!("../../templates/app/src/app/auth/register/register.component.html");
const FORGOT_PASSWORD_COMPONENT_TS: &str =
    include_str!("../../templates/app/src/app/auth/forgot-password/forgot-password.component.ts");
const FORGOT_PASSWORD_COMPONENT_HTML: &str =
    include_str!("../../templates/app/src/app/auth/forgot-password/forgot-password.component.html");
const RESET_PASSWORD_COMPONENT_TS: &str =
    include_str!("../../templates/app/src/app/auth/reset-password/reset-password.component.ts");
const RESET_PASSWORD_COMPONENT_HTML: &str =
    include_str!("../../templates/app/src/app/auth/reset-password/reset-password.component.html");
const VERIFY_EMAIL_COMPONENT_TS: &str =
    include_str!("../../templates/app/src/app/auth/verify-email/verify-email.component.ts");
const VERIFY_EMAIL_COMPONENT_HTML: &str =
    include_str!("../../templates/app/src/app/auth/verify-email/verify-email.component.html");
const HOME_PAGE_TS: &str = include_str!("../../templates/app/src/app/home/home.page.ts");
const HOME_PAGE_HTML: &str = include_str!("../../templates/app/src/app/home/home.page.html");
const APP_CAPACITOR_CONFIG_TS: &str = include_str!("../../templates/app/capacitor.config.ts");
const WWW_PACKAGE_JSON: &str = include_str!("../../templates/www/package.json");
const WWW_ASTRO_CONFIG: &str = include_str!("../../templates/www/astro.config.mjs");
const WWW_TSCONFIG: &str = include_str!("../../templates/www/tsconfig.json");
const WWW_ENV_D_TS: &str = include_str!("../../templates/www/src/env.d.ts");
const WWW_LAYOUT: &str = include_str!("../../templates/www/src/layouts/Layout.astro");
const WWW_INDEX: &str = include_str!("../../templates/www/src/pages/index.astro");
const WWW_GLOBAL_CSS: &str = include_str!("../../templates/www/src/styles/global.css");
const WWW_FAVICON: &str = include_str!("../../templates/www/public/favicon.svg");
const E2E_PLAYWRIGHT: &str = include_str!("../../templates/e2e/playwright.config.ts");
const E2E_HEALTH: &str = include_str!("../../templates/e2e/health.spec.ts");
const E2E_PACKAGE_JSON: &str = include_str!("../../templates/e2e/package.json");

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn handle_new(
    name: &str,
    path: Option<&str>,
    erno_path: Option<&str>,
    bundle_id: Option<&str>,
    start_dev: bool,
    no_dev: bool,
) -> ui::Cmd {
    validate_name(name);

    let dest = match path {
        Some(p) => std::path::PathBuf::from(p).join(name),
        None => std::path::PathBuf::from(name),
    };

    if dest.exists() {
        return Err(format!("directory '{}' already exists", dest.display()).into());
    }

    let (erno_dep, erno_angular_dep) = resolve_erno_deps(erno_path);
    let jwt_secret = generate_jwt_secret();
    let db_name = name.replace('-', "_");
    let db_password = db_name.clone();
    // Capacitor bundle IDs must not contain dashes; replace with underscores.
    let bundle_id = bundle_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("com.example.{}", name.replace('-', "_")));

    ui::section(ui::icon::NEW, format!("Creating {name}"));
    ui::detail(dest.display().to_string());

    ui::section(ui::icon::PACKAGE, "Scaffolding");

    let angular_version = erno_path.and_then(read_angular_version_from_dist);

    create_api(&dest, name, &db_name, &jwt_secret, &db_password, &erno_dep);
    ionic_new_app(name, &bundle_id, &dest);
    patch_app(
        &dest,
        name,
        &bundle_id,
        &erno_angular_dep,
        angular_version.as_deref(),
        erno_path,
    );
    // ionic start installs base deps before we patch package.json, so
    // erno-angular and other additions are not yet in node_modules.
    install_app_deps(&dest, erno_angular_dep.starts_with("file:"));
    create_www(&dest, name);
    install_www_deps(&dest);
    create_e2e(&dest);
    copy_admin(&dest, erno_path);
    let has_monitoring = copy_monitoring(&dest, erno_path, name, &db_name);

    ui::section(ui::icon::DATABASE, "Databases");

    let config = GlobalConfig::load().ok();
    if let Some(config) = config {
        create_databases(
            &config.postgres.admin_url,
            &db_name,
            &db_password,
            has_monitoring,
        )
        .await;
    } else {
        ui::warn("Skipped database creation — no ~/.erno/config.toml found");
        ui::detail(format!(
            "Run `erno setup`, then create them manually:\n\
             createdb {db_name}_development\n\
             createdb {db_name}_test"
        ));
    }

    let start = decide_start_dev(
        start_dev,
        no_dev,
        std::io::IsTerminal::is_terminal(&std::io::stdin()),
    )?;

    print_next_steps(name, start);
    if start {
        crate::commands::dev::handle_dev(Some(dest), crate::commands::dev::DevArgs::default())
            .await?;
    }
    Ok(())
}

pub fn decide_start_dev(dev: bool, no_dev: bool, is_tty: bool) -> Result<bool, String> {
    if dev && no_dev {
        return Err("cannot combine --dev and --no-dev".into());
    }
    if no_dev {
        return Ok(false);
    }
    if dev {
        return Ok(true);
    }
    if !is_tty {
        return Ok(false);
    }
    Ok(ui::confirm("Start dev servers now?", true))
}

// ── Erno dependency resolution ────────────────────────────────────────────────

fn resolve_erno_deps(erno_path: Option<&str>) -> (String, String) {
    const ERNO_GIT: &str = "https://github.com/tomekpiotrowski/erno";
    match erno_path {
        Some(p) => {
            let (repo_root, api_path) = resolve_local_erno_paths(p);
            let angular_dist = repo_root.join("app/dist/erno-angular");
            if !angular_dist.join("package.json").is_file() {
                ui::abort(&format!(
                    "could not find a built erno-angular package at {}\n\
                     Run `cd {}/app && npm install && npm run build -- erno-angular` first.",
                    angular_dist.display(),
                    repo_root.display()
                ));
            }
            (
                format!(r#"{{ path = "{}" }}"#, api_path.display()),
                format!("file:{}", angular_dist.display()),
            )
        }
        None => (format!(r#"{{ git = "{ERNO_GIT}" }}"#), "^0.0.1".to_string()),
    }
}

fn resolve_local_erno_paths(path: &str) -> (PathBuf, PathBuf) {
    // Both dependency strings are written into files under the generated
    // project, so a relative --erno-path would be resolved against the wrong
    // directory by cargo and npm. Make it absolute before anything else.
    let input = fs::canonicalize(path).unwrap_or_else(|e| {
        ui::abort(&format!("invalid --erno-path '{path}': {e}"));
    });
    let input = input.as_path();

    let is_api_path = input
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == "api")
        .unwrap_or(false)
        && input.join("Cargo.toml").is_file();

    let (repo_root, api_path) = if is_api_path {
        let Some(repo_root) = input.parent() else {
            ui::abort(&format!(
                "invalid --erno-path '{}': the api directory has no parent",
                input.display()
            ));
        };
        (repo_root.to_path_buf(), input.to_path_buf())
    } else {
        (input.to_path_buf(), input.join("api"))
    };

    if !api_path.join("Cargo.toml").is_file() {
        ui::abort(&format!(
            "invalid --erno-path '{}': could not find {}",
            input.display(),
            api_path.join("Cargo.toml").display()
        ));
    }

    (repo_root, api_path)
}

// ── Validation ────────────────────────────────────────────────────────────────

fn validate_name(name: &str) {
    let valid = !name.is_empty()
        && name
            .chars()
            .next()
            .map(|c| c.is_ascii_lowercase())
            .unwrap_or(false)
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if !valid {
        ui::abort(&format!("invalid name '{name}'. Use lowercase letters, digits, hyphens, or underscores (must start with a letter)"));
    }
}

// ── JWT secret ────────────────────────────────────────────────────────────────

fn generate_jwt_secret() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ── File helpers ──────────────────────────────────────────────────────────────

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            ui::abort(&format!(
                "failed to create directory {}: {e}",
                parent.display()
            ));
        });
    }
    fs::write(path, content).unwrap_or_else(|e| {
        ui::abort(&format!("failed to write {}: {e}", path.display()));
    });
}

fn with_test_utils_feature(dep: &str) -> String {
    let trimmed = dep.trim();
    if let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        format!("{{ {}, features = [\"test-utils\"] }}", inner.trim())
    } else {
        format!("{trimmed}, features = [\"test-utils\"]")
    }
}

fn render(template: &str, vars: &[(&str, &str)]) -> String {
    vars.iter().fold(template.to_string(), |s, (k, v)| {
        s.replace(&format!("{{{{{k}}}}}"), v)
    })
}

// ── Rust API scaffold ─────────────────────────────────────────────────────────

fn create_api(
    dest: &Path,
    name: &str,
    db_name: &str,
    jwt_secret: &str,
    db_password: &str,
    erno_dep: &str,
) {
    let api = dest.join("api");
    let crate_name = name.replace('-', "_");
    let erno_dep_test = with_test_utils_feature(erno_dep);

    write(
        &api.join("Cargo.toml"),
        &render(
            API_CARGO_TOML,
            &[
                ("name", name),
                ("erno_dep", erno_dep),
                ("erno_dep_test", &erno_dep_test),
            ],
        ),
    );
    write(
        &api.join("src/lib.rs"),
        &render(API_LIB_RS, &[("name", name)]),
    );
    write(
        &api.join("src/main.rs"),
        &render(API_MAIN_RS, &[("crate_name", &crate_name)]),
    );
    write(
        &api.join("tests/common/mod.rs"),
        &render(API_TESTS_COMMON, &[("crate_name", &crate_name)]),
    );
    write(&api.join("tests/health.rs"), API_TESTS_HEALTH);
    write(&api.join("src/migrations/mod.rs"), API_MIGRATIONS_MOD_RS);
    write(
        &api.join("config/development.toml"),
        &render(
            API_DEVELOPMENT_TOML,
            &[
                ("db_name", db_name),
                ("db_password", db_password),
                ("jwt_secret", jwt_secret),
            ],
        ),
    );
    write(
        &api.join("config/production.toml"),
        &render(API_PRODUCTION_TOML, &[("db_name", db_name)]),
    );
    write(
        &api.join("config/test.toml"),
        &render(
            API_TEST_TOML,
            &[("db_name", db_name), ("db_password", db_password)],
        ),
    );
    write(&dest.join(".gitignore"), GITIGNORE);
}

// ── Install app npm dependencies ─────────────────────────────────────────────

fn install_app_deps(dest: &Path, use_install_links: bool) {
    let app = dest.join("app");
    let mut cmd = std::process::Command::new("npm");
    cmd.arg("install");
    if use_install_links {
        // file: directory deps are symlinked by default; --install-links copies
        // them instead, which avoids the duplicate Angular runtime (NG0203).
        cmd.arg("--install-links");
    }
    let status = cmd.current_dir(&app).status().unwrap_or_else(|e| {
        ui::abort(&format!("failed to run npm install: {e}"));
    });
    if !status.success() {
        ui::abort("npm install failed");
    }
    ui::ok("app dependencies");
}

// ── Marketing site (Astro static) ─────────────────────────────────────────────

fn create_e2e(dest: &Path) {
    let e2e = dest.join("e2e");
    write(&e2e.join("package.json"), E2E_PACKAGE_JSON);
    write(&e2e.join("playwright.config.ts"), E2E_PLAYWRIGHT);
    write(&e2e.join("health.spec.ts"), E2E_HEALTH);
    ui::ok("Playwright e2e tests (e2e/)");
}

fn create_www(dest: &Path, name: &str) {
    let www = dest.join("www");

    write(
        &www.join("package.json"),
        &render(WWW_PACKAGE_JSON, &[("name", name)]),
    );
    write(&www.join("astro.config.mjs"), WWW_ASTRO_CONFIG);
    write(&www.join("tsconfig.json"), WWW_TSCONFIG);
    write(&www.join("src/env.d.ts"), WWW_ENV_D_TS);
    write(&www.join("src/layouts/Layout.astro"), WWW_LAYOUT);
    write(
        &www.join("src/pages/index.astro"),
        &render(WWW_INDEX, &[("name", name)]),
    );
    write(&www.join("src/styles/global.css"), WWW_GLOBAL_CSS);
    write(&www.join("public/favicon.svg"), WWW_FAVICON);
    ui::ok("marketing site (www/)");
}

fn copy_admin(dest: &Path, erno_path: Option<&str>) {
    let Some(erno_path) = erno_path else {
        return;
    };
    let src = PathBuf::from(erno_path).join("admin");
    if !src.join("package.json").is_file() {
        return;
    }
    let dst = dest.join("admin");
    copy_dir_filtered(&src, &dst);
    ui::ok("admin/");
}

/// Copy the monitoring deployment into the new project.
///
/// Unlike [`copy_admin`], this runs whether or not `--erno-path` was given: the
/// monitoring stack is part of every project now. Without an explicit path it
/// falls back to the checkout this CLI was built from, which covers anyone
/// working from the monorepo. Returns whether it actually landed.
fn copy_monitoring(dest: &Path, erno_path: Option<&str>, name: &str, db_name: &str) -> bool {
    let Some(src) = framework_root(erno_path).map(|r| r.join("monitoring")) else {
        ui::warn("could not find an erno checkout to copy monitoring/ from");
        ui::detail(
            "The project is complete without it. To add it later, re-run with\n\
             --erno-path <path-to-erno>, or copy monitoring/ across by hand.",
        );
        return false;
    };
    if !src.join("Cargo.toml").is_file() {
        return false;
    }

    let dst = dest.join("monitoring");
    copy_dir_filtered(&src, &dst);
    rewrite_monitoring_manifest(&dst, erno_path);
    rewrite_monitoring_config(&dst, name, db_name);
    ui::ok("monitoring/");
    true
}

/// Where the framework's own `admin/` and `monitoring/` trees live.
///
/// `--erno-path` wins. Otherwise fall back to the checkout this binary was
/// compiled from, which is present for anyone developing against the monorepo
/// and absent for `cargo install erno-cli` users — hence the `Option`.
fn framework_root(erno_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = erno_path {
        return Some(resolve_local_erno_paths(p).0);
    }
    let built_from = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?;
    built_from
        .join("monitoring/Cargo.toml")
        .is_file()
        .then(|| built_from.to_path_buf())
}

/// Point the copied `monitoring/Cargo.toml` at erno the same way `api/` does.
///
/// Rewritten line-wise rather than by replacing the literal `"../api"`: the git
/// form carries no path at all, and `--erno-path` yields an absolute one. A
/// relative `../api` would also escape the Docker build context, so getting
/// this wrong breaks both `cargo check` and the image build.
fn rewrite_monitoring_manifest(dir: &Path, erno_path: Option<&str>) {
    let path = dir.join("Cargo.toml");
    let Ok(content) = fs::read_to_string(&path) else {
        return;
    };
    let (erno_dep, _) = resolve_erno_deps(erno_path);
    let dev_dep = with_test_utils_feature(&erno_dep);

    let mut out = Vec::new();
    let mut in_dev = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_dev = trimmed == "[dev-dependencies]";
        }
        if trimmed.starts_with("erno") && trimmed.contains("path") {
            out.push(format!(
                "erno = {}",
                if in_dev { &dev_dep } else { &erno_dep }
            ));
            continue;
        }
        out.push(line.to_string());
    }
    let _ = fs::write(path, out.join("\n") + "\n");
}

/// Give the collector its own databases, named after this project.
///
/// Without this every project on a machine shares `erno_monitoring`, silently
/// mixing their error data.
fn rewrite_monitoring_config(dir: &Path, name: &str, db_name: &str) {
    for (file, suffix) in [
        ("development.toml", "monitoring_development"),
        ("test.toml", "monitoring_test"),
    ] {
        let path = dir.join("config").join(file);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let updated = content
            .replace("erno_monitoring_test", &format!("{db_name}_{suffix}"))
            .replace("erno_monitoring", &format!("{db_name}_{suffix}"))
            .replace("Erno status", &format!("{name} status"));
        let _ = fs::write(path, updated);
    }
}

fn copy_dir_filtered(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).ok();
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        // `target` and `.git` matter for monitoring/, which carries a Rust
        // crate: copying a build directory means gigabytes.
        if name == "node_modules"
            || name == "dist"
            || name == ".angular"
            || name == "target"
            || name == ".git"
        {
            continue;
        }
        let from = entry.path();
        let to = dst.join(name);
        if from.is_dir() {
            copy_dir_filtered(&from, &to);
        } else {
            let _ = std::fs::copy(&from, &to);
        }
    }
}

fn install_www_deps(dest: &Path) {
    let www = dest.join("www");
    let status = std::process::Command::new("npm")
        .arg("install")
        .current_dir(&www)
        .status()
        .unwrap_or_else(|e| {
            ui::abort(&format!("failed to run npm install in www/: {e}"));
        });
    if !status.success() {
        ui::abort("npm install failed in www/");
    }
    ui::ok("www dependencies");
}

// ── Ionic app scaffold (via ionic start) ──────────────────────────────────────

fn ionic_new_app(_name: &str, _bundle_id: &str, dest: &Path) {
    let ionic = match crate::ng::find_ionic_binary() {
        Some(p) => p,
        None => {
            ui::abort("Ionic CLI not found\nInstall it with: npm install -g @ionic/cli");
        }
    };

    let status = Command::new(ionic)
        .args([
            "start",
            "app",
            "blank",
            "--type=angular-standalone",
            "--no-deps",
            "--no-git",
        ])
        .env("CI", "true")
        .env("NG_CLI_ANALYTICS", "false")
        .current_dir(dest)
        .status()
        .unwrap_or_else(|e| {
            ui::abort(&format!("failed to run ionic start: {e}"));
        });

    if !status.success() {
        ui::abort("ionic start failed");
    }
    ui::ok("Ionic app");
}

// ── Read Angular version required by local erno-angular dist ─────────────────

fn read_angular_version_from_dist(erno_path: &str) -> Option<String> {
    let (repo_root, _) = resolve_local_erno_paths(erno_path);
    let dist_pkg = repo_root.join("app/dist/erno-angular/package.json");
    let content = fs::read_to_string(dist_pkg).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;
    pkg["peerDependencies"]["@angular/core"]
        .as_str()
        .map(|s| s.to_string())
}

// ── Patch Angular app with erno-specific changes ──────────────────────────────

fn patch_app(
    dest: &Path,
    name: &str,
    bundle_id: &str,
    erno_angular_dep: &str,
    angular_version: Option<&str>,
    erno_path: Option<&str>,
) {
    let app = dest.join("app");

    let pkg_path = app.join("package.json");
    let pkg_content = fs::read_to_string(&pkg_path).unwrap_or_else(|e| {
        ui::abort(&format!("failed to read package.json: {e}"));
    });
    let mut pkg: serde_json::Value = serde_json::from_str(&pkg_content).unwrap_or_else(|e| {
        ui::abort(&format!("failed to parse package.json: {e}"));
    });

    pkg["name"] = serde_json::Value::String(format!("{name}-app"));
    pkg["dependencies"]["erno-angular"] = serde_json::Value::String(erno_angular_dep.to_string());
    pkg["scripts"]["test:ci"] = serde_json::Value::String("ng test --watch=false".to_string());

    // Capacitor — added here rather than via `ionic start --capacitor` to avoid
    // that step running bun install, which conflicts with our npm-only workflow.
    pkg["dependencies"]["@capacitor/core"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["dependencies"]["@capacitor/app"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["dependencies"]["@capacitor/haptics"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["dependencies"]["@capacitor/keyboard"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["dependencies"]["@capacitor/status-bar"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["devDependencies"]["@capacitor/cli"] = serde_json::Value::String("^7.0.0".to_string());
    // `erno dev --ios/--android` shells out to `ionic cap run`; vendoring the CLI
    // keeps that working without a global install or an npx fetch.
    pkg["devDependencies"]["@ionic/cli"] = serde_json::Value::String("^7.2.0".to_string());

    // When erno-angular is installed as a symlink (file: directory dep), npm does
    // not hoist its dependencies into the consumer's node_modules. Inject them
    // here so they are present alongside the symlink.
    if let Some(ep) = erno_path {
        let (repo_root, _) = resolve_local_erno_paths(ep);
        let lib_pkg_path = repo_root.join("app/dist/erno-angular/package.json");
        if let Ok(content) = fs::read_to_string(&lib_pkg_path) {
            if let Ok(lib_pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(deps) = lib_pkg["dependencies"].as_object() {
                    for (dep_name, dep_ver) in deps {
                        // Angular packages are already present in the app; skip them.
                        if dep_name.starts_with("@angular/") {
                            continue;
                        }
                        // Only insert if not already declared by the app.
                        if pkg["dependencies"][dep_name].is_null() {
                            pkg["dependencies"][dep_name] = dep_ver.clone();
                        }
                    }
                }
            }
        }
    }

    // Pin @angular/* versions to match what erno-angular was compiled against,
    // overriding whatever ng new chose based on the globally installed CLI.
    if let Some(ver) = angular_version {
        for section in ["dependencies", "devDependencies"] {
            if let Some(map) = pkg[section].as_object_mut() {
                for (key, val) in map.iter_mut() {
                    if key.starts_with("@angular/") {
                        *val = serde_json::Value::String(ver.to_string());
                    }
                }
            }
        }
    }

    write(
        &pkg_path,
        &(serde_json::to_string_pretty(&pkg).unwrap() + "\n"),
    );

    // When erno-angular is a symlink, the bundler (esbuild) by default follows
    // symlinks and resolves imports from the real path — finding Angular in the
    // Erno workspace's node_modules instead of the app's, which loads two
    // Angular runtimes and causes NG0203. Setting preserveSymlinks=true tells
    // esbuild to resolve from the symlink location (the app's node_modules),
    // so only one Angular runtime is ever loaded.
    if erno_angular_dep.starts_with("file:") {
        let angular_json_path = app.join("angular.json");
        if let Ok(aj_content) = fs::read_to_string(&angular_json_path) {
            if let Ok(mut aj) = serde_json::from_str::<serde_json::Value>(&aj_content) {
                let build_opts = &mut aj["projects"]["app"]["architect"]["build"]["options"];
                build_opts["preserveSymlinks"] = serde_json::Value::Bool(true);
                write(
                    &angular_json_path,
                    &(serde_json::to_string_pretty(&aj).unwrap() + "\n"),
                );
            }
        }
    }

    // Replace ionic-generated files with erno versions.
    write(&app.join("src/main.ts"), APP_MAIN_TS);
    write(&app.join("src/app/app.component.ts"), APP_COMPONENT_TS);
    write(
        &app.join("src/environments/environment.ts"),
        APP_ENVIRONMENT_TS,
    );
    write(
        &app.join("src/environments/environment.prod.ts"),
        APP_ENVIRONMENT_PROD_TS,
    );
    write(&app.join("src/app/app.component.html"), APP_COMPONENT_HTML);
    write(&app.join("src/app/app.routes.ts"), APP_ROUTES_TS);
    write(&app.join("src/app/auth/auth.guard.ts"), AUTH_GUARD_TS);
    write(
        &app.join("src/app/auth/login/login.component.ts"),
        LOGIN_COMPONENT_TS,
    );
    write(
        &app.join("src/app/auth/login/login.component.html"),
        LOGIN_COMPONENT_HTML,
    );
    write(
        &app.join("src/app/auth/register/register.component.ts"),
        REGISTER_COMPONENT_TS,
    );
    write(
        &app.join("src/app/auth/register/register.component.html"),
        REGISTER_COMPONENT_HTML,
    );
    write(
        &app.join("src/app/auth/forgot-password/forgot-password.component.ts"),
        FORGOT_PASSWORD_COMPONENT_TS,
    );
    write(
        &app.join("src/app/auth/forgot-password/forgot-password.component.html"),
        FORGOT_PASSWORD_COMPONENT_HTML,
    );
    write(
        &app.join("src/app/auth/reset-password/reset-password.component.ts"),
        RESET_PASSWORD_COMPONENT_TS,
    );
    write(
        &app.join("src/app/auth/reset-password/reset-password.component.html"),
        RESET_PASSWORD_COMPONENT_HTML,
    );
    write(
        &app.join("src/app/auth/verify-email/verify-email.component.ts"),
        VERIFY_EMAIL_COMPONENT_TS,
    );
    write(
        &app.join("src/app/auth/verify-email/verify-email.component.html"),
        VERIFY_EMAIL_COMPONENT_HTML,
    );
    write(&app.join("src/app/home/home.page.ts"), HOME_PAGE_TS);
    write(&app.join("src/app/home/home.page.html"), HOME_PAGE_HTML);
    write(
        &app.join("capacitor.config.ts"),
        &render(
            APP_CAPACITOR_CONFIG_TS,
            &[("bundle_id", bundle_id), ("name", name)],
        ),
    );
}

// ── Database creation ─────────────────────────────────────────────────────────

async fn create_databases(
    admin_url: &str,
    db_name: &str,
    db_password: &str,
    with_monitoring: bool,
) {
    match tokio_postgres::connect(admin_url, tokio_postgres::NoTls).await {
        Err(e) => {
            ui::warn(format!(
                "could not connect to PostgreSQL to create databases: {e}"
            ));
            ui::detail(format!(
                "Create them manually:\n\
                 createuser {db_name}\n\
                 createdb -O {db_name} {db_name}_development\n\
                 createdb -O {db_name} {db_name}_test"
            ));
        }
        Ok((client, connection)) => {
            tokio::spawn(async move {
                let _ = connection.await;
            });
            if create_db_user(&client, db_name, db_password).await {
                let mut databases =
                    vec![format!("{db_name}_development"), format!("{db_name}_test")];
                // The collector keeps its own, deliberately separate from the
                // application's — in production it is a different deployment
                // on different infrastructure.
                if with_monitoring {
                    databases.push(format!("{db_name}_monitoring_development"));
                    databases.push(format!("{db_name}_monitoring_test"));
                }
                for db in &databases {
                    create_db(&client, db).await;
                    grant_schema_public(admin_url, db, db_name).await;
                }
            }
        }
    }
}

async fn create_db_user(client: &tokio_postgres::Client, name: &str, password: &str) -> bool {
    let sql = format!(
        "DO $$ BEGIN \
         IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{name}') THEN \
         CREATE USER {name} WITH PASSWORD '{password}'; \
         END IF; \
         END $$"
    );
    match client.execute(&sql, &[]).await {
        Ok(_) => {
            ui::ok(format!("user {name}"));
            true
        }
        Err(e) => {
            let msg = e
                .as_db_error()
                .map(|d| d.message().to_string())
                .unwrap_or_else(|| e.to_string());
            ui::warn(format!("could not create user {name}: {msg}"));
            ui::detail("Grant CREATEROLE to your admin user and re-run, or run `erno doctor`.");
            false
        }
    }
}

fn with_db(admin_url: &str, db: &str) -> String {
    match admin_url.rfind('/') {
        Some(pos) => format!("{}/{}", &admin_url[..pos], db),
        None => format!("{}/{}", admin_url, db),
    }
}

async fn grant_schema_public(admin_url: &str, db: &str, user: &str) {
    match tokio_postgres::connect(&with_db(admin_url, db), tokio_postgres::NoTls).await {
        Ok((client, connection)) => {
            tokio::spawn(async move {
                let _ = connection.await;
            });
            match client
                .execute(&format!("GRANT ALL ON SCHEMA public TO {user}"), &[])
                .await
            {
                Ok(_) => ui::ok(format!("granted schema permissions to {user} on {db}")),
                Err(e) => ui::warn(format!("could not grant schema permissions on {db}: {e}")),
            }
        }
        Err(e) => ui::warn(format!(
            "could not connect to {db} to grant permissions: {e}"
        )),
    }
}

async fn create_db(client: &tokio_postgres::Client, db: &str) {
    match client.execute(&format!("CREATE DATABASE {db}"), &[]).await {
        Ok(_) => ui::ok(db),
        Err(e) => {
            let msg = e
                .as_db_error()
                .map(|d| d.message())
                .unwrap_or("unknown error");
            if msg.contains("already exists") {
                ui::info(format!("{db} already exists"));
            } else {
                ui::warn(format!("could not create {db}: {msg}"));
            }
        }
    }
}

// ── Next steps ────────────────────────────────────────────────────────────────

fn print_next_steps(name: &str, starting_dev: bool) {
    ui::section(ui::icon::DONE, format!("Created {name}/"));
    ui::blank();
    ui::info("api/         Rust API");
    ui::info("app/         Ionic product app (app.example.com in production)");
    ui::info("www/         Astro marketing site (example.com in production)");
    ui::info("admin/       Operator console for the API (admin.example.com)");
    ui::info("monitoring/  Error reporting, uptime, alerts, status page");
    ui::detail("Deployed separately: erno deploy init --target monitoring");
    ui::blank();

    if starting_dev {
        ui::finished(
            ui::icon::DEV,
            "Starting the dev servers — Ctrl+C to stop. \
             The API applies pending migrations on boot.",
        );
        return;
    }
    ui::next_steps(
        "Next steps",
        &[
            format!("cd {name}"),
            "erno test".to_string(),
            "erno dev".to_string(),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::{decide_start_dev, rewrite_monitoring_config, rewrite_monitoring_manifest};
    use std::fs;

    /// A temp dir keyed by a nonce, matching the pattern used elsewhere in the
    /// CLI's tests, with manual cleanup.
    fn scratch(nonce: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("erno-new-{nonce}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("config")).unwrap();
        dir
    }

    const MANIFEST: &str = r#"[package]
name = "erno-monitoring"

[dependencies]
erno = { path = "../api" }
axum = { workspace = true }

[dev-dependencies]
erno = { path = "../api", features = ["test-utils"] }
axum-test = { workspace = true }
"#;

    #[test]
    fn the_erno_dependency_is_rewritten_for_a_generated_project() {
        let dir = scratch("manifest-git");
        fs::write(dir.join("Cargo.toml"), MANIFEST).unwrap();

        rewrite_monitoring_manifest(&dir, None);
        let out = fs::read_to_string(dir.join("Cargo.toml")).unwrap();

        // `../api` would not exist in a generated project, and would escape the
        // Docker build context even if it did.
        assert!(!out.contains("../api"), "{out}");
        assert!(out.contains(r#"erno = { git = "https://github.com/tomekpiotrowski/erno" }"#));
        // The dev-dependency keeps test-utils, or the collector's suite will
        // not compile.
        assert!(out.contains("test-utils"), "{out}");
        // Untouched lines survive.
        assert!(out.contains("axum = { workspace = true }"));
        assert!(out.contains("axum-test = { workspace = true }"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_collector_gets_its_own_databases_named_after_the_project() {
        let dir = scratch("config");
        fs::write(
            dir.join("config/development.toml"),
            "[database]\nurl = \"postgres://erno:erno@localhost/erno_monitoring\"\nname = \"Erno status\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("config/test.toml"),
            "[database]\nurl = \"postgres://erno:erno@localhost/erno_monitoring_test\"\n",
        )
        .unwrap();

        rewrite_monitoring_config(&dir, "acme", "acme");

        let dev = fs::read_to_string(dir.join("config/development.toml")).unwrap();
        let test = fs::read_to_string(dir.join("config/test.toml")).unwrap();
        // Two projects on one machine must not share an error database.
        assert!(dev.contains("acme_monitoring_development"), "{dev}");
        assert!(test.contains("acme_monitoring_test"), "{test}");
        assert!(!dev.contains("erno_monitoring"), "{dev}");
        assert!(!test.contains("erno_monitoring"), "{test}");
        assert!(dev.contains("acme status"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn flags_override_tty() {
        assert!(decide_start_dev(true, false, false).unwrap());
        assert!(!decide_start_dev(false, true, true).unwrap());
        assert!(decide_start_dev(true, true, true).is_err());
    }

    #[test]
    fn non_tty_does_not_start_without_flag() {
        assert!(!decide_start_dev(false, false, false).unwrap());
    }
}
