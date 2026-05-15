use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;
use rand::Rng;

use crate::global_config::GlobalConfig;

// ── Embedded templates ────────────────────────────────────────────────────────

const GITIGNORE: &str = include_str!("../../templates/.gitignore");
const API_CARGO_TOML: &str = include_str!("../../templates/api/Cargo.toml");
const API_MAIN_RS: &str = include_str!("../../templates/api/src/main.rs");
const API_MIGRATIONS_MOD_RS: &str = include_str!("../../templates/api/src/migrations/mod.rs");
const API_DEVELOPMENT_TOML: &str = include_str!("../../templates/api/config/development.toml");
const API_PRODUCTION_TOML: &str = include_str!("../../templates/api/config/production.toml");
const API_TEST_TOML: &str = include_str!("../../templates/api/config/test.toml");
const APP_MODULE_TS: &str = include_str!("../../templates/app/app.module.ts");
const APP_COMPONENT_HTML: &str = include_str!("../../templates/app/app.component.html");
const APP_ROUTING_MODULE_TS: &str = include_str!("../../templates/app/src/app/app-routing.module.ts");
const AUTH_GUARD_TS: &str = include_str!("../../templates/app/src/app/auth/auth.guard.ts");
const LOGIN_COMPONENT_TS: &str = include_str!("../../templates/app/src/app/auth/login/login.component.ts");
const LOGIN_COMPONENT_HTML: &str = include_str!("../../templates/app/src/app/auth/login/login.component.html");
const REGISTER_COMPONENT_TS: &str = include_str!("../../templates/app/src/app/auth/register/register.component.ts");
const REGISTER_COMPONENT_HTML: &str = include_str!("../../templates/app/src/app/auth/register/register.component.html");
const FORGOT_PASSWORD_COMPONENT_TS: &str = include_str!("../../templates/app/src/app/auth/forgot-password/forgot-password.component.ts");
const FORGOT_PASSWORD_COMPONENT_HTML: &str = include_str!("../../templates/app/src/app/auth/forgot-password/forgot-password.component.html");
const RESET_PASSWORD_COMPONENT_TS: &str = include_str!("../../templates/app/src/app/auth/reset-password/reset-password.component.ts");
const RESET_PASSWORD_COMPONENT_HTML: &str = include_str!("../../templates/app/src/app/auth/reset-password/reset-password.component.html");
const VERIFY_EMAIL_COMPONENT_TS: &str = include_str!("../../templates/app/src/app/auth/verify-email/verify-email.component.ts");
const VERIFY_EMAIL_COMPONENT_HTML: &str = include_str!("../../templates/app/src/app/auth/verify-email/verify-email.component.html");
const HOME_PAGE_TS: &str = include_str!("../../templates/app/src/app/home/home.page.ts");
const HOME_PAGE_HTML: &str = include_str!("../../templates/app/src/app/home/home.page.html");
const APP_CAPACITOR_CONFIG_TS: &str = include_str!("../../templates/app/capacitor.config.ts");

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn handle_new(name: &str, path: Option<&str>, erno_path: Option<&str>, bundle_id: Option<&str>) {
    validate_name(name);

    let dest = match path {
        Some(p) => std::path::PathBuf::from(p).join(name),
        None => std::path::PathBuf::from(name),
    };

    if dest.exists() {
        eprintln!("❌  Directory '{}' already exists.", dest.display());
        std::process::exit(1);
    }

    let (erno_dep, erno_angular_dep) = resolve_erno_deps(erno_path);
    let jwt_secret = generate_jwt_secret();
    let db_name = name.replace('-', "_");
    let db_password = db_name.clone();
    // Capacitor bundle IDs must not contain dashes; replace with underscores.
    let bundle_id = bundle_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("com.example.{}", name.replace('-', "_")));

    println!("Creating {}...", dest.display());

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
    // The ionic blank template runs `ionic integrations enable capacitor` which
    // uses bun regardless of flags. Remove the bun lockfile and node_modules so
    // our npm install below produces a clean, npm-only install.
    let app_dir = dest.join("app");
    let _ = fs::remove_file(app_dir.join("bun.lockb"));
    let _ = fs::remove_dir_all(app_dir.join("node_modules"));
    install_app_deps(&dest, erno_angular_dep.starts_with("file:"));

    let config = GlobalConfig::load().ok();
    if let Some(config) = config {
        create_databases(&config.postgres.admin_url, &db_name, &db_password).await;
    } else {
        println!(
            "\n  ⚠️   Skipped database creation — no ~/.erno/config.toml found.\n      Run `erno setup` then create databases manually:\n        createdb {db_name}_development\n        createdb {db_name}_test"
        );
    }

    print_next_steps(name, &dest);
    crate::commands::dev::handle_dev(Some(dest)).await;
}

// ── Erno dependency resolution ────────────────────────────────────────────────

fn resolve_erno_deps(erno_path: Option<&str>) -> (String, String) {
    const ERNO_GIT: &str = "https://github.com/tomekpiotrowski/erno";
    match erno_path {
        Some(p) => {
            let (repo_root, api_path) = resolve_local_erno_paths(p);
            let angular_dist = repo_root.join("app/dist/erno-angular");
            if !angular_dist.join("package.json").is_file() {
                eprintln!(
                    "❌  Could not find a built erno-angular package at {}.\n    Run `cd {}/app && npm install && npm run build -- erno-angular` first.",
                    angular_dist.display(),
                    repo_root.display()
                );
                std::process::exit(1);
            }
            (
                format!(r#"{{ path = "{}", features = ["admin"] }}"#, api_path.display()),
                format!("file:{}", angular_dist.display()),
            )
        }
        None => (
            format!(r#"{{ git = "{ERNO_GIT}", features = ["admin"] }}"#),
            "^0.0.1".to_string(),
        ),
    }
}

fn resolve_local_erno_paths(path: &str) -> (PathBuf, PathBuf) {
    let input = Path::new(path);
    let is_api_path = input
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == "api")
        .unwrap_or(false)
        && input.join("Cargo.toml").is_file();

    let (repo_root, api_path) = if is_api_path {
        let Some(repo_root) = input.parent() else {
            eprintln!(
                "❌  Invalid --erno-path '{}': api directory has no parent.",
                input.display()
            );
            std::process::exit(1);
        };
        (repo_root.to_path_buf(), input.to_path_buf())
    } else {
        (input.to_path_buf(), input.join("api"))
    };

    if !api_path.join("Cargo.toml").is_file() {
        eprintln!(
            "❌  Invalid --erno-path '{}': could not find {}.",
            input.display(),
            api_path.join("Cargo.toml").display()
        );
        std::process::exit(1);
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
        eprintln!(
            "❌  Invalid name '{name}'. Use lowercase letters, digits, hyphens, or underscores (must start with a letter)."
        );
        std::process::exit(1);
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
            eprintln!("❌  Failed to create directory {}: {e}", parent.display());
            std::process::exit(1);
        });
    }
    fs::write(path, content).unwrap_or_else(|e| {
        eprintln!("❌  Failed to write {}: {e}", path.display());
        std::process::exit(1);
    });
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

    write(
        &api.join("Cargo.toml"),
        &render(API_CARGO_TOML, &[("name", name), ("erno_dep", erno_dep)]),
    );
    write(
        &api.join("src/main.rs"),
        &render(API_MAIN_RS, &[("name", name)]),
    );
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
    println!("  Installing app dependencies...");
    let mut cmd = std::process::Command::new("npm");
    cmd.arg("install");
    if use_install_links {
        // file: directory deps are symlinked by default; --install-links copies
        // them instead, which avoids the duplicate Angular runtime (NG0203).
        cmd.arg("--install-links");
    }
    let status = cmd.current_dir(&app).status().unwrap_or_else(|e| {
        eprintln!("❌  Failed to run npm install: {e}");
        std::process::exit(1);
    });
    if !status.success() {
        eprintln!("❌  npm install failed");
        std::process::exit(1);
    }
}

// ── Ionic app scaffold (via ionic start) ──────────────────────────────────────

fn ionic_new_app(_name: &str, _bundle_id: &str, dest: &Path) {
    let ionic = match crate::ng::find_ionic_binary() {
        Some(p) => p,
        None => {
            eprintln!("❌  Ionic CLI not found. Run: npm install -g @ionic/cli");
            std::process::exit(1);
        }
    };

    println!("  Scaffolding Ionic app...");

    let status = Command::new(ionic)
        .args([
            "start",
            "app",
            "blank",
            "--type=angular",
            "--no-deps",
            "--no-git",
        ])
        .env("CI", "true")
        .env("NG_CLI_ANALYTICS", "false")
        .current_dir(dest)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("❌  Failed to run ionic start: {e}");
            std::process::exit(1);
        });

    if !status.success() {
        eprintln!("❌  ionic start failed");
        std::process::exit(1);
    }
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
        eprintln!("❌  Failed to read package.json: {e}");
        std::process::exit(1);
    });
    let mut pkg: serde_json::Value = serde_json::from_str(&pkg_content).unwrap_or_else(|e| {
        eprintln!("❌  Failed to parse package.json: {e}");
        std::process::exit(1);
    });

    pkg["name"] = serde_json::Value::String(format!("{name}-app"));
    pkg["dependencies"]["erno-angular"] = serde_json::Value::String(erno_angular_dep.to_string());

    // Capacitor — added here rather than via `ionic start --capacitor` to avoid
    // that step running bun install, which conflicts with our npm-only workflow.
    pkg["dependencies"]["@capacitor/core"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["dependencies"]["@capacitor/app"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["dependencies"]["@capacitor/haptics"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["dependencies"]["@capacitor/keyboard"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["dependencies"]["@capacitor/status-bar"] = serde_json::Value::String("^7.0.0".to_string());
    pkg["devDependencies"]["@capacitor/cli"] = serde_json::Value::String("^7.0.0".to_string());

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
                // vitest is an optional peer of @angular/build; the version
                // added by ng new may not match the pinned @angular/build.
                map.remove("vitest");
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

    // ionic start generates `import 'zone.js'` in polyfills.ts; comment it out
    // so Angular runs in zoneless mode (provideZonelessChangeDetection() in app.module.ts).
    let polyfills_path = app.join("src/polyfills.ts");
    if let Ok(content) = fs::read_to_string(&polyfills_path) {
        let patched = content.replace("import 'zone.js';", "// import 'zone.js';");
        write(&polyfills_path, &patched);
    }

    // ionic start generates a lazily-loaded home page with its own module; remove those
    // files since we use eager routing with HomeComponent declared directly in AppModule.
    let _ = fs::remove_file(app.join("src/app/home/home.module.ts"));
    let _ = fs::remove_file(app.join("src/app/home/home-routing.module.ts"));

    // Replace ionic-generated files with erno versions
    write(&app.join("src/app/app.module.ts"), APP_MODULE_TS);
    write(&app.join("src/app/app.component.html"), APP_COMPONENT_HTML);
    write(&app.join("src/app/app-routing.module.ts"), APP_ROUTING_MODULE_TS);
    write(&app.join("src/app/auth/auth.guard.ts"), AUTH_GUARD_TS);
    write(&app.join("src/app/auth/login/login.component.ts"), LOGIN_COMPONENT_TS);
    write(&app.join("src/app/auth/login/login.component.html"), LOGIN_COMPONENT_HTML);
    write(&app.join("src/app/auth/register/register.component.ts"), REGISTER_COMPONENT_TS);
    write(&app.join("src/app/auth/register/register.component.html"), REGISTER_COMPONENT_HTML);
    write(&app.join("src/app/auth/forgot-password/forgot-password.component.ts"), FORGOT_PASSWORD_COMPONENT_TS);
    write(&app.join("src/app/auth/forgot-password/forgot-password.component.html"), FORGOT_PASSWORD_COMPONENT_HTML);
    write(&app.join("src/app/auth/reset-password/reset-password.component.ts"), RESET_PASSWORD_COMPONENT_TS);
    write(&app.join("src/app/auth/reset-password/reset-password.component.html"), RESET_PASSWORD_COMPONENT_HTML);
    write(&app.join("src/app/auth/verify-email/verify-email.component.ts"), VERIFY_EMAIL_COMPONENT_TS);
    write(&app.join("src/app/auth/verify-email/verify-email.component.html"), VERIFY_EMAIL_COMPONENT_HTML);
    write(&app.join("src/app/home/home.page.ts"), HOME_PAGE_TS);
    write(&app.join("src/app/home/home.page.html"), HOME_PAGE_HTML);
    write(
        &app.join("capacitor.config.ts"),
        &render(APP_CAPACITOR_CONFIG_TS, &[("bundle_id", bundle_id), ("name", name)]),
    );
}

// ── Database creation ─────────────────────────────────────────────────────────

async fn create_databases(admin_url: &str, db_name: &str, db_password: &str) {
    match tokio_postgres::connect(admin_url, tokio_postgres::NoTls).await {
        Err(e) => {
            println!("\n  ⚠️   Could not connect to PostgreSQL to create databases ({e}).");
            println!("      Create them manually:");
            println!("        createuser {db_name}");
            println!("        createdb -O {db_name} {db_name}_development");
            println!("        createdb -O {db_name} {db_name}_test");
        }
        Ok((client, connection)) => {
            tokio::spawn(async move {
                let _ = connection.await;
            });
            if create_db_user(&client, db_name, db_password).await {
                create_db(&client, &format!("{db_name}_development")).await;
                grant_schema_public(admin_url, &format!("{db_name}_development"), db_name).await;
                create_db(&client, &format!("{db_name}_test")).await;
                grant_schema_public(admin_url, &format!("{db_name}_test"), db_name).await;
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
            println!("  ✅  Created database user {name}");
            true
        }
        Err(e) => {
            let msg = e
                .as_db_error()
                .map(|d| d.message().to_string())
                .unwrap_or_else(|| e.to_string());
            println!("  ⚠️   Could not create user {name}: {msg}");
            println!("      Grant CREATEROLE to your admin user and re-run, or run `erno doctor`.");
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
                Ok(_) => println!("  ✅  Granted schema permissions to {user} on {db}"),
                Err(e) => println!("  ⚠️   Could not grant schema permissions on {db}: {e}"),
            }
        }
        Err(e) => println!("  ⚠️   Could not connect to {db} to grant permissions: {e}"),
    }
}

async fn create_db(client: &tokio_postgres::Client, db: &str) {
    match client
        .execute(&format!("CREATE DATABASE {db}"), &[])
        .await
    {
        Ok(_) => println!("  ✅  Created database {db}"),
        Err(e) => {
            let msg = e
                .as_db_error()
                .map(|d| d.message())
                .unwrap_or("unknown error");
            if msg.contains("already exists") {
                println!("  ℹ️   Database {db} already exists");
            } else {
                println!("  ⚠️   Could not create {db}: {msg}");
            }
        }
    }
}

// ── Next steps ────────────────────────────────────────────────────────────────

fn print_next_steps(name: &str, dest: &Path) {
    println!(
        r#"
✅  Created {name}/

Before the API connects, run migrations once:
  cd {dest}/api && cargo run -- migrate up

Starting dev servers now (Ctrl+C to stop)...
"#,
        dest = dest.display(),
    );
}
