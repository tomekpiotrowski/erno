use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;
use rand::Rng;

use crate::global_config::GlobalConfig;
use crate::ui;

// ── Embedded templates ────────────────────────────────────────────────────────

const GITIGNORE: &str = include_str!("../../templates/.gitignore");
// `.template`, not `Cargo.toml`: cargo's git installer walks every
// Cargo.toml in the repo, and the `{{erno_dep}}` placeholders are not valid TOML.
const API_CARGO_TOML: &str = include_str!("../../templates/api/Cargo.toml.template");
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
const CI_WORKFLOW: &str = include_str!("../../templates/github/workflows/ci.yml");

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
    );
    // ionic start installs base deps before we patch package.json, so
    // erno-angular and other additions are not yet in node_modules.
    install_app_deps(&dest);
    create_www(&dest, name);
    install_www_deps(&dest);
    create_e2e(&dest);
    install_e2e_deps(&dest);
    write_ci_workflow(&dest, name);
    ui::ok("GitHub Actions CI (.github/workflows/ci.yml)");
    copy_admin(&dest, erno_path);

    ui::section(ui::icon::DATABASE, "Databases");

    let config = GlobalConfig::load().ok();
    if let Some(config) = config {
        create_databases(&config.postgres.admin_url, &db_name, &db_password).await;
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
    match erno_path {
        Some(p) => {
            let (repo_root, api_path) = resolve_local_erno_paths(p);
            let tarball = pack_local_erno_angular(&repo_root);
            (
                format!(r#"{{ path = "{}" }}"#, api_path.display()),
                format!("file:{}", tarball.display()),
            )
        }
        None => (
            crate::version::erno_git_dep(),
            crate::version::erno_angular_tarball_url(),
        ),
    }
}

/// Pack `app/dist/erno-angular` so the generated app depends on a tarball
/// rather than a directory symlink (which pulls in a second Angular runtime).
fn pack_local_erno_angular(repo_root: &Path) -> PathBuf {
    let dist = repo_root.join("app/dist/erno-angular");
    let pkg_path = dist.join("package.json");
    if !pkg_path.is_file() {
        ui::abort(&format!(
            "could not find a built erno-angular package at {}\n\
             Run `cd {}/app && npm install && npm run build -- erno-angular` first.",
            dist.display(),
            repo_root.display()
        ));
    }
    let version = fs::read_to_string(&pkg_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|pkg| pkg.get("version")?.as_str().map(str::to_string))
        .unwrap_or_else(|| {
            ui::abort(&format!(
                "could not read version from {}",
                pkg_path.display()
            ))
        });
    let status = Command::new("npm")
        .arg("pack")
        .current_dir(&dist)
        .status()
        .unwrap_or_else(|e| ui::abort(&format!("failed to run npm pack: {e}")));
    if !status.success() {
        ui::abort("npm pack of erno-angular failed");
    }
    let tarball = dist.join(format!("erno-angular-{version}.tgz"));
    if !tarball.is_file() {
        ui::abort(&format!("npm pack did not write {}", tarball.display()));
    }
    ui::ok(format!(
        "packed {}",
        tarball.file_name().unwrap().to_string_lossy()
    ));
    tarball
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
    rustfmt_api(&api);
}

/// rustfmt the generated crate so `cargo fmt --check` is green for any
/// project name. Import order of `erno` vs the crate itself is alphabetical
/// under stable rustfmt, so a single template order cannot satisfy both
/// `acme` and `teryon`.
fn rustfmt_api(api: &Path) {
    let status = Command::new("cargo")
        .args(["fmt", "--all"])
        .current_dir(api)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(_) | Err(_) => ui::warn(
            "could not rustfmt the generated API — install rustfmt (`rustup component add rustfmt`)",
        ),
    }
}

// ── Install app npm dependencies ─────────────────────────────────────────────

fn install_app_deps(dest: &Path) {
    let app = dest.join("app");
    let mut cmd = std::process::Command::new("npm");
    cmd.arg("install");
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

fn install_e2e_deps(dest: &Path) {
    let e2e = dest.join("e2e");
    let status = Command::new("npm")
        .arg("install")
        .current_dir(&e2e)
        .status()
        .unwrap_or_else(|e| {
            ui::abort(&format!("failed to run npm install in e2e/: {e}"));
        });
    if !status.success() {
        ui::abort("npm install failed in e2e/");
    }
    ui::ok("e2e dependencies");
}

/// Render and write `.github/workflows/ci.yml` for a freshly scaffolded app.
///
/// Extracted so unit tests can check the file without running `ionic start`.
fn write_ci_workflow(dest: &Path, name: &str) {
    write(
        &dest.join(".github/workflows/ci.yml"),
        &render_ci_workflow(name),
    );
}

fn render_ci_workflow(name: &str) -> String {
    let db_name = name.replace('-', "_");
    render(CI_WORKFLOW, &[("name", name), ("db_name", &db_name)])
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

fn copy_dir_filtered(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).ok();
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        // Build output and history are never part of a scaffold.
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
    // Ionic 9's published ESM uses extensionless relative imports that
    // Vitest/jsdom cannot resolve, so `ng test --watch=false` is red on a
    // fresh app. Drop the scripts so `erno test` stays green; re-add them
    // when that is fixed.
    if let Some(scripts) = pkg["scripts"].as_object_mut() {
        scripts.remove("test");
        scripts.remove("test:ci");
    }

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

async fn create_databases(admin_url: &str, db_name: &str, db_password: &str) {
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
                let databases = [format!("{db_name}_development"), format!("{db_name}_test")];
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
    ui::blank();
    ui::info("Error reporting, uptime and alerts come from a collector this app");
    ui::detail(
        "does not contain: one deployment watches every Erno app in an\n\
         organisation. Register this one with it to start reporting.",
    );
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
    use super::{decide_start_dev, resolve_erno_deps};
    use std::process::Command;

    fn scratch_dir(prefix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

    #[test]
    fn default_deps_are_tagged_git_and_tarball() {
        let (dep, angular) = resolve_erno_deps(None);
        let v = env!("CARGO_PKG_VERSION");
        assert!(dep.contains("git = "), "{dep}");
        assert!(dep.contains(&format!("tag = \"v{v}\"")), "{dep}");
        assert_eq!(
            angular,
            format!(
                "https://github.com/tomekpiotrowski/erno/releases/download/v{v}/erno-angular-{v}.tgz"
            )
        );
    }

    #[test]
    fn rendered_ci_workflow_mirrors_local_checks() {
        let yaml = super::render_ci_workflow("acme");
        assert!(yaml.contains("POSTGRES_USER: acme"), "{yaml}");
        assert!(yaml.contains("acme_test"), "{yaml}");
        assert!(yaml.contains("npm ci"), "{yaml}");
        assert!(yaml.contains("cargo fmt --all --check"), "{yaml}");
        assert!(yaml.contains("cargo clippy"), "{yaml}");
        assert!(yaml.contains("needs: api"), "{yaml}");
        assert!(yaml.contains("pg_isready -U acme"), "{yaml}");
        assert!(
            yaml.contains("cargo build"),
            "e2e must compile before booting: {yaml}"
        );
        assert!(
            yaml.contains("./target/debug/acme"),
            "e2e must start the compiled binary: {yaml}"
        );
        assert!(
            !yaml.contains("bun ci")
                && !yaml.contains("bun install")
                && !yaml.contains("setup-bun"),
            "generated CI must use npm, not bun:\n{yaml}"
        );
        assert!(
            !yaml
                .lines()
                .any(|line| line.trim().starts_with("run:") && line.contains("test:ci")),
            "do not ship a red app unit-test job:\n{yaml}"
        );
    }

    #[test]
    fn ci_workflow_uses_underscored_postgres_role_for_hyphenated_names() {
        let yaml = super::render_ci_workflow("ci-smoke");
        assert!(yaml.contains("POSTGRES_USER: ci_smoke"), "{yaml}");
        assert!(yaml.contains("pg_isready -U ci_smoke"), "{yaml}");
        assert!(yaml.contains("./target/debug/ci-smoke"), "{yaml}");
        assert!(!yaml.contains("POSTGRES_USER: ci-smoke"), "{yaml}");
    }

    #[test]
    fn write_ci_workflow_creates_the_file() {
        let dir = scratch_dir("erno-ci-yml");
        super::write_ci_workflow(&dir, "acme");
        let path = dir.join(".github/workflows/ci.yml");
        let yaml = std::fs::read_to_string(&path).unwrap();
        assert!(path.is_file());
        assert!(yaml.contains("POSTGRES_USER: acme"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gitignore_ignores_playwright_output() {
        let gitignore = include_str!("../../templates/.gitignore");
        for path in [
            "/e2e/node_modules",
            "/e2e/test-results",
            "/e2e/playwright-report",
        ] {
            assert!(
                gitignore.lines().any(|line| line.trim() == path),
                "generated .gitignore must ignore {path}"
            );
        }
    }

    #[test]
    fn generated_api_sources_pass_cargo_fmt() {
        // Dummy-render the templates into a crate named `acme` so rustfmt sees
        // the same crate-vs-external grouping `cargo fmt --check` does in a
        // fresh app. `{{crate_name}}` is not valid Rust until it is replaced.
        let dir = scratch_dir("erno-api-fmt");
        std::fs::create_dir_all(dir.join("src/migrations")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"acme\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            include_str!("../../templates/api/src/lib.rs").replace("{{name}}", "acme"),
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            include_str!("../../templates/api/src/main.rs").replace("{{crate_name}}", "acme"),
        )
        .unwrap();
        std::fs::write(
            dir.join("src/migrations/mod.rs"),
            include_str!("../../templates/api/src/migrations/mod.rs"),
        )
        .unwrap();

        let output = Command::new("cargo")
            .args(["fmt", "--all", "--check"])
            .current_dir(&dir)
            .output()
            .expect("cargo fmt");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            output.status.success(),
            "generated api sources must pass cargo fmt --check\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn rustfmt_api_fixes_import_order_for_names_after_erno() {
        // `widget` sorts after `erno`, so the template's crate-then-erno
        // imports fail `cargo fmt --check` until rustfmt_api rewrites them.
        let dir = scratch_dir("erno-api-fmt-widget");
        std::fs::create_dir_all(dir.join("src/migrations")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"widget\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            include_str!("../../templates/api/src/lib.rs").replace("{{name}}", "widget"),
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            include_str!("../../templates/api/src/main.rs").replace("{{crate_name}}", "widget"),
        )
        .unwrap();
        std::fs::write(
            dir.join("src/migrations/mod.rs"),
            include_str!("../../templates/api/src/migrations/mod.rs"),
        )
        .unwrap();

        super::rustfmt_api(&dir);

        let output = Command::new("cargo")
            .args(["fmt", "--all", "--check"])
            .current_dir(&dir)
            .output()
            .expect("cargo fmt");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            output.status.success(),
            "rustfmt_api must leave a crate named widget cargo-fmt-clean\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
