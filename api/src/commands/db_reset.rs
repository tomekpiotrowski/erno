use std::{error::Error, process};

use sea_orm::{ConnectOptions, ConnectionTrait, Database};

use crate::config::Config;

/// Handles the database reset command.
///
/// Drops every table and type in the current database, then runs all
/// migrations. The database itself is kept: `DROP DATABASE` needs to own the
/// database (or be superuser), but the app role that ran the migrations
/// already owns the objects it created.
pub async fn handle_db_reset_command<AppMigrator: sea_orm_migration::MigratorTrait, ExtraConfig>(
    config: &Config<ExtraConfig>,
) {
    if let Err(e) = reset_database::<AppMigrator, ExtraConfig>(config).await {
        eprintln!("❌ Database reset failed: {e}");
        process::exit(1);
    }
}

async fn reset_database<AppMigrator: sea_orm_migration::MigratorTrait, ExtraConfig>(
    config: &Config<ExtraConfig>,
) -> Result<(), Box<dyn Error>> {
    println!("🔄 Resetting database (this will drop all tables!)...");

    let mut opt = ConnectOptions::new(config.database.url.clone());
    opt.max_connections(1);
    opt.sqlx_logging(false);
    let db = Database::connect(opt).await?;

    // Other sessions (a running API, leftover pool connections) can hold
    // locks that block DROP TABLE. Same-role backends are fair game.
    let _ = db
        .execute_unprepared(
            "SELECT pg_terminate_backend(pid) \
             FROM pg_stat_activity \
             WHERE datname = current_database() \
             AND pid <> pg_backend_pid()",
        )
        .await;

    println!("Dropping all tables and reapplying migrations...");
    AppMigrator::fresh(&db).await?;

    println!("✅ Database reset completed successfully");

    Ok(())
}
