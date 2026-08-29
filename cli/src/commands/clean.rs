use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use std::process::Command;

use clap::Args;
use tokio_postgres::NoTls;

use crate::commands::dev::{parse_table_string, resolve_project_root, running_pid};
use crate::commands::packages::{load_packages, Package};
use crate::global_config::GlobalConfig;
use crate::ui;

const ARTIFACTS: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    ".angular",
    ".astro",
    "test-results",
    "playwright-report",
];

const CONVENTIONAL_DIRS: &[&str] = &["api", "app", "www", "admin", "e2e"];

/// Package directories from the manifest, plus the conventional scaffold dirs
/// so `admin/` is still cleaned when `erno.toml` omitted it.
fn package_bases(packages: &[Package]) -> BTreeSet<String> {
    let mut bases: BTreeSet<String> = CONVENTIONAL_DIRS.iter().map(|s| (*s).to_string()).collect();
    for package in packages {
        if !package.dir.is_empty() {
            bases.insert(package.dir.clone());
        }
    }
    bases
}

/// Project-relative artifact directories that exist and are safe to delete.
fn collect_dirs(root: &Path, packages: &[Package]) -> Vec<String> {
    let mut found = BTreeSet::new();
    if root.join(".erno").is_dir() {
        found.insert(".erno".to_string());
    }
    for base in package_bases(packages) {
        for artifact in ARTIFACTS {
            let rel = format!("{base}/{artifact}");
            if root.join(&rel).is_dir() {
                found.insert(rel);
            }
        }
    }
    found.into_iter().collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedDatabase {
    name: String,
    user: Option<String>,
}

fn is_postgres_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn database_name(url: &str) -> Option<String> {
    let after = url.rsplit('/').next()?;
    let name = after.split('?').next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn database_user(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let creds = rest.split_once('@')?.0;
    let user = creds.split(':').next()?.trim();
    if user.is_empty() {
        None
    } else {
        Some(user.to_string())
    }
}

fn drop_sql(name: &str) -> String {
    format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")
}

fn create_sql(name: &str) -> String {
    format!("CREATE DATABASE {name}")
}

fn grant_sql(_name: &str, user: &str) -> String {
    format!("GRANT ALL ON SCHEMA public TO {user}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Proceed {
    Yes,
    Ask,
}

fn remove_dirs(root: &Path, rels: &[String]) -> Vec<Result<String, String>> {
    rels.iter()
        .map(|rel| {
            let path = root.join(rel);
            match fs::remove_dir_all(&path) {
                Ok(()) => Ok(rel.clone()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(rel.clone()),
                Err(e) => Err(format!("{rel}: {e}")),
            }
        })
        .collect()
}

fn should_proceed(yes: bool, is_tty: bool) -> Result<Proceed, String> {
    if yes {
        return Ok(Proceed::Yes);
    }
    if !is_tty {
        return Err("refusing to clean without --yes in a non-interactive terminal".into());
    }
    Ok(Proceed::Ask)
}

fn confirm_question(dirs: usize, dbs: usize) -> &'static str {
    match (dirs > 0, dbs > 0) {
        (true, true) => "Remove these artifacts and drop the local databases?",
        (true, false) => "Remove these artifacts?",
        (false, true) => "Drop the local databases?",
        (false, false) => "Continue?",
    }
}

fn count_label(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn applied_summary(dirs: usize, dbs: usize) -> String {
    match (dirs, dbs) {
        (0, n) => format!("reset {}", count_label(n, "database", "databases")),
        (n, 0) => format!("removed {}", count_label(n, "artifact", "artifacts")),
        (d, b) => format!(
            "removed {}, reset {}",
            count_label(d, "artifact", "artifacts"),
            count_label(b, "database", "databases")
        ),
    }
}

fn postgres_ready() -> Result<(), String> {
    match Command::new("pg_isready").status() {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err("PostgreSQL is not running (`pg_isready` failed)\n\
             Start it — e.g.: sudo service postgresql start"
            .into()),
        Err(_) => Err("PostgreSQL client tools not found (`pg_isready`)\n\
             Install PostgreSQL: https://www.postgresql.org/download/"
            .into()),
    }
}

fn with_db(admin_url: &str, db: &str) -> String {
    match admin_url.rfind('/') {
        Some(pos) => format!("{}/{}", &admin_url[..pos], db),
        None => format!("{admin_url}/{db}"),
    }
}

fn db_err(error: &tokio_postgres::Error) -> String {
    error
        .as_db_error()
        .map(|d| d.message().to_string())
        .unwrap_or_else(|| error.to_string())
}

async fn connect(url: &str) -> Result<tokio_postgres::Client, String> {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

async fn drop_database(client: &tokio_postgres::Client, name: &str) -> Result<(), String> {
    if client.execute(&drop_sql(name), &[]).await.is_ok() {
        return Ok(());
    }
    client
        .execute(&format!("DROP DATABASE IF EXISTS {name}"), &[])
        .await
        .map(|_| ())
        .map_err(|e| db_err(&e))
}

async fn grant_public(admin_url: &str, db: &str, user: &str) -> Result<(), String> {
    let client = connect(&with_db(admin_url, db)).await?;
    client
        .execute(&grant_sql(db, user), &[])
        .await
        .map(|_| ())
        .map_err(|e| db_err(&e))
}

async fn reset_databases(admin_url: &str, dbs: &[PlannedDatabase]) -> Result<(), String> {
    let client = connect(admin_url)
        .await
        .map_err(|e| format!("could not connect as admin: {e}"))?;
    let mut failed = false;
    for db in dbs {
        if let Err(e) = drop_database(&client, &db.name).await {
            ui::warn(format!("could not drop {}: {e}", db.name));
            failed = true;
            continue;
        }
        match client.execute(&create_sql(&db.name), &[]).await {
            Ok(_) => ui::ok(&db.name),
            Err(e) => {
                ui::warn(format!("could not create {}: {}", db.name, db_err(&e)));
                failed = true;
                continue;
            }
        }
        if let Some(user) = &db.user {
            if let Err(e) = grant_public(admin_url, &db.name, user).await {
                ui::warn(format!("could not grant schema on {}: {e}", db.name));
            }
        }
    }
    if failed {
        Err("could not reset every local database".into())
    } else {
        Ok(())
    }
}

fn collect_databases(root: &Path, packages: &[Package]) -> Result<Vec<PlannedDatabase>, String> {
    let mut found: BTreeMap<String, PlannedDatabase> = BTreeMap::new();
    for base in package_bases(packages) {
        for file in ["development.toml", "test.toml"] {
            let path = root.join(&base).join("config").join(file);
            let Ok(raw) = fs::read_to_string(&path) else {
                continue;
            };
            let Some(url) = parse_table_string(&raw, "database", "url") else {
                continue;
            };
            let Some(name) = database_name(&url) else {
                continue;
            };
            if !is_postgres_ident(&name) {
                return Err(format!(
                    "database name `{name}` in {} is not a Postgres identifier",
                    path.display()
                ));
            }
            let user = match database_user(&url) {
                Some(user) if is_postgres_ident(&user) => Some(user),
                Some(user) => {
                    return Err(format!(
                        "database user `{user}` in {} is not a Postgres identifier",
                        path.display()
                    ));
                }
                None => None,
            };
            found
                .entry(name.clone())
                .or_insert(PlannedDatabase { name, user });
        }
    }
    Ok(found.into_values().collect())
}

#[derive(Args, Debug, Default)]
pub struct CleanArgs {
    /// Print the plan and exit
    #[arg(long)]
    pub dry_run: bool,
    /// Run without prompting
    #[arg(long)]
    pub yes: bool,
}

pub async fn handle_clean(args: CleanArgs) -> ui::Cmd {
    let root = resolve_project_root(None)?;
    if let Some(pid) = running_pid(&root) {
        return Err(format!("erno dev is already running (pid {pid}). Stop it first.").into());
    }

    let packages = load_packages(&root)?;
    let dirs = collect_dirs(&root, &packages);
    let dbs = collect_databases(&root, &packages)?;

    ui::section(ui::icon::CLEAN, "Clean");
    if dirs.is_empty() && dbs.is_empty() {
        ui::finished(ui::icon::DONE, "already clean");
        return Ok(());
    }
    ui::blank();
    for dir in &dirs {
        ui::info(dir);
    }
    for db in &dbs {
        ui::info(&db.name);
    }

    if args.dry_run {
        ui::finished(
            ui::icon::DONE,
            format!("dry run: {}", applied_summary(dirs.len(), dbs.len())),
        );
        return Ok(());
    }

    match should_proceed(args.yes, std::io::stdin().is_terminal())? {
        Proceed::Yes => {}
        Proceed::Ask => {
            if !ui::confirm(confirm_question(dirs.len(), dbs.len()), false) {
                ui::warn("cancelled");
                return Ok(());
            }
        }
    }

    if !dbs.is_empty() {
        postgres_ready()?;
        let admin_url = GlobalConfig::load()
            .map_err(|_| {
                "local databases are configured but ~/.erno/config.toml was not found.\n\
                 Run `erno setup`, then retry."
                    .to_string()
            })?
            .postgres
            .admin_url;
        reset_databases(&admin_url, &dbs).await?;
    }

    let mut dir_failed = false;
    for result in remove_dirs(&root, &dirs) {
        match result {
            Ok(rel) => ui::ok(rel),
            Err(e) => {
                ui::warn(e);
                dir_failed = true;
            }
        }
    }
    if dir_failed {
        return Err("could not remove every artifact".into());
    }

    ui::finished(ui::icon::DONE, applied_summary(dirs.len(), dbs.len()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::commands::packages::Package;

    fn temp(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "erno-clean-{}-{}-{suffix}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pkg(name: &str, dir: &str) -> Package {
        Package {
            name: name.into(),
            dir: dir.into(),
            default: true,
            database: false,
            kind: None,
            build: Vec::new(),
            lint: Vec::new(),
            test: Vec::new(),
            dev: Vec::new(),
        }
    }

    fn touch_dir(root: &Path, rel: &str) {
        fs::create_dir_all(root.join(rel)).unwrap();
    }

    fn touch_file(root: &Path, rel: &str, contents: &str) {
        if let Some(parent) = root.join(rel).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(root.join(rel), contents).unwrap();
    }

    #[test]
    fn empty_tree_has_nothing_to_clean() {
        let root = temp("empty");
        let found = collect_dirs(&root, &[]);
        assert!(found.is_empty(), "{found:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn includes_erno_dir_at_the_project_root() {
        let root = temp("erno-dir");
        touch_dir(&root, ".erno");
        assert_eq!(collect_dirs(&root, &[]), vec![".erno"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn includes_known_artifacts_when_they_exist() {
        let root = temp("artifacts");
        for rel in [
            "api/target",
            "app/node_modules",
            "app/dist",
            "app/.angular",
            "www/.astro",
            "e2e/test-results",
        ] {
            touch_dir(&root, rel);
        }
        assert_eq!(
            collect_dirs(&root, &[]),
            vec![
                "api/target",
                "app/.angular",
                "app/dist",
                "app/node_modules",
                "e2e/test-results",
                "www/.astro",
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn does_not_collect_source_or_config() {
        let root = temp("source");
        touch_file(&root, "api/src/lib.rs", "fn main() {}");
        touch_file(&root, ".env", "SECRET=1");
        touch_file(&root, "config/local.toml", "");
        touch_file(&root, "api/config/development.toml", "[database]\n");
        touch_dir(&root, "api/src");
        assert!(collect_dirs(&root, &[pkg("api", "api")]).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn includes_opt_in_package_dirs_from_the_manifest() {
        let root = temp("opt-in");
        touch_dir(&root, "vision/target");
        let mut vision = pkg("vision", "vision");
        vision.default = false;
        assert_eq!(collect_dirs(&root, &[vision]), vec!["vision/target"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn includes_admin_even_when_the_manifest_omits_it() {
        let root = temp("admin");
        touch_dir(&root, "admin/node_modules");
        touch_dir(&root, "admin/dist");
        assert_eq!(
            collect_dirs(&root, &[pkg("api", "api")]),
            vec!["admin/dist", "admin/node_modules"]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn names_are_project_relative_and_sorted() {
        let root = temp("sorted");
        touch_dir(&root, ".erno");
        touch_dir(&root, "www/dist");
        touch_dir(&root, "api/target");
        assert_eq!(
            collect_dirs(&root, &[]),
            vec![".erno", "api/target", "www/dist"]
        );
        let _ = fs::remove_dir_all(&root);
    }

    fn write_db_url(root: &Path, rel: &str, url: &str) {
        touch_file(root, rel, &format!("[database]\nurl = \"{url}\"\n"));
    }

    #[test]
    fn reads_database_urls_from_development_and_test_config() {
        let root = temp("db-urls");
        write_db_url(
            &root,
            "api/config/development.toml",
            "postgres://app:secret@localhost/app_development",
        );
        write_db_url(
            &root,
            "api/config/test.toml",
            "postgres://app:secret@localhost/app_test",
        );
        let dbs = collect_databases(&root, &[pkg("api", "api")]).unwrap();
        let names: Vec<_> = dbs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["app_development", "app_test"]);
        assert_eq!(dbs[0].user.as_deref(), Some("app"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn deduplicates_the_same_database_name() {
        let root = temp("db-dedup");
        write_db_url(
            &root,
            "api/config/development.toml",
            "postgres://app:secret@localhost/app_dev",
        );
        write_db_url(
            &root,
            "api/config/test.toml",
            "postgres://app:secret@localhost/app_dev",
        );
        let dbs = collect_databases(&root, &[pkg("api", "api")]).unwrap();
        assert_eq!(dbs.len(), 1);
        assert_eq!(dbs[0].name, "app_dev");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parses_role_from_database_url() {
        assert_eq!(
            database_user("postgres://cubeast_erno:secret@localhost/cubeast_erno_test").as_deref(),
            Some("cubeast_erno")
        );
        assert_eq!(
            database_name("postgres://cubeast_erno:secret@localhost/cubeast_erno_test").as_deref(),
            Some("cubeast_erno_test")
        );
    }

    #[test]
    fn rejects_unsafe_database_identifiers() {
        for name in ["foo-bar", "", "erno;drop", "has space"] {
            assert!(!is_postgres_ident(name), "{name} should be rejected");
        }
        assert!(is_postgres_ident("erno_dev"));
        assert!(is_postgres_ident("_tmp"));
        assert!(is_postgres_ident("A1"));

        let root = temp("db-bad");
        write_db_url(
            &root,
            "api/config/development.toml",
            "postgres://app:secret@localhost/foo-bar",
        );
        let err = collect_databases(&root, &[pkg("api", "api")]).unwrap_err();
        assert!(err.contains("foo-bar"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn drop_and_create_sql_use_the_validated_name() {
        assert_eq!(
            drop_sql("erno_dev"),
            "DROP DATABASE IF EXISTS erno_dev WITH (FORCE)"
        );
        assert_eq!(create_sql("erno_dev"), "CREATE DATABASE erno_dev");
        assert_eq!(
            grant_sql("erno_dev", "app"),
            "GRANT ALL ON SCHEMA public TO app"
        );
    }

    #[test]
    fn yes_flag_skips_the_prompt() {
        assert_eq!(should_proceed(true, false).unwrap(), Proceed::Yes);
        assert_eq!(should_proceed(true, true).unwrap(), Proceed::Yes);
    }

    #[test]
    fn a_tty_without_yes_asks() {
        assert_eq!(should_proceed(false, true).unwrap(), Proceed::Ask);
    }

    #[test]
    fn a_non_tty_without_yes_refuses() {
        let err = should_proceed(false, false).unwrap_err();
        assert!(err.contains("--yes"), "{err}");
    }

    #[test]
    fn confirm_question_names_what_will_be_removed() {
        assert!(confirm_question(1, 1).contains("artifacts"));
        assert!(confirm_question(1, 1).contains("databases"));
        assert_eq!(confirm_question(2, 0), "Remove these artifacts?");
        assert_eq!(confirm_question(0, 2), "Drop the local databases?");
    }

    #[test]
    fn applied_summary_counts_artifacts_and_databases() {
        assert_eq!(applied_summary(1, 0), "removed 1 artifact");
        assert_eq!(
            applied_summary(3, 2),
            "removed 3 artifacts, reset 2 databases"
        );
        assert_eq!(applied_summary(0, 1), "reset 1 database");
    }

    #[test]
    fn remove_dirs_deletes_artifacts_and_leaves_source() {
        let root = temp("remove");
        touch_file(&root, ".erno/dev.log", "log");
        touch_file(&root, "api/target/foo", "obj");
        touch_file(&root, "api/src/lib.rs", "fn main() {}");

        let results = remove_dirs(&root, &[".erno".into(), "api/target".into()]);
        assert!(results.iter().all(|r| r.is_ok()), "{results:?}");
        assert!(!root.join(".erno").exists());
        assert!(!root.join("api/target").exists());
        assert_eq!(
            fs::read_to_string(root.join("api/src/lib.rs")).unwrap(),
            "fn main() {}"
        );

        let missing = remove_dirs(&root, &["app/node_modules".into()]);
        assert!(missing.iter().all(|r| r.is_ok()), "{missing:?}");
        let _ = fs::remove_dir_all(&root);
    }
}
