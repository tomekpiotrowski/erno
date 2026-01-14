use std::{error::Error, process};

use sea_orm::{ConnectOptions, ConnectionTrait, Database, DbBackend, Statement};
use tracing::{debug, info};

use crate::{cli::MigrateAction, config::Config};

/// Handles the database reset command.
///
/// Drops and recreates the database, then runs all migrations. This provides
/// a completely clean database state. This command connects to the postgres
/// database to drop/create the target database.
pub async fn handle_db_reset_command<AppMigrator: sea_orm_migration::MigratorTrait>(
    config: &Config,
) {
    if let Err(e) = reset_database::<AppMigrator>(config).await {
        eprintln!("❌ Database reset failed: {e}");
        process::exit(1);
    }
}

async fn reset_database<AppMigrator: sea_orm_migration::MigratorTrait>(
    config: &Config,
) -> Result<(), Box<dyn Error>> {
    info!("🔄 Resetting database (this will drop and recreate the database!)...");

    // Parse the database URL to extract connection details
    // Expected format: postgresql://user:pass@host:port/dbname
    let db_url = &config.database.url;
    let db_name = db_url
        .split('/')
        .next_back()
        .ok_or("Database name not found in URL")?
        .split('?')
        .next()
        .ok_or("Invalid URL format")?;

    if db_name.is_empty() {
        return Err("Database name not found in URL".into());
    }

    debug!("Database name: {}", db_name);

    // Create a URL for the postgres database (used to drop/create the target database)
    let postgres_url = db_url.replace(&format!("/{}", db_name), "/postgres");

    debug!("Connecting to postgres database");

    // Connect to the postgres database
    let mut opt = ConnectOptions::new(postgres_url);
    opt.max_connections(1);
    let postgres_db = Database::connect(opt).await?;

    // Fix any collation version mismatches in template databases
    info!("Checking collation versions...");
    let fix_collation_sql = "ALTER DATABASE template1 REFRESH COLLATION VERSION";
    let _ = postgres_db
        .execute(Statement::from_string(
            DbBackend::Postgres,
            fix_collation_sql,
        ))
        .await; // Ignore errors if this fails

    // Terminate all existing connections to the target database
    info!(
        "Terminating existing connections to database '{}'...",
        db_name
    );
    let terminate_sql = format!(
        "SELECT pg_terminate_backend(pg_stat_activity.pid) \
         FROM pg_stat_activity \
         WHERE pg_stat_activity.datname = '{}' \
         AND pid <> pg_backend_pid()",
        db_name
    );
    postgres_db
        .execute(Statement::from_string(DbBackend::Postgres, terminate_sql))
        .await?;

    // Drop the database if it exists
    info!("Dropping database '{}'...", db_name);
    let drop_sql = format!("DROP DATABASE IF EXISTS \"{}\"", db_name);
    postgres_db
        .execute(Statement::from_string(DbBackend::Postgres, drop_sql))
        .await?;

    // Create the database
    info!("Creating database '{}'...", db_name);
    let create_sql = format!("CREATE DATABASE \"{}\"", db_name);
    postgres_db
        .execute(Statement::from_string(DbBackend::Postgres, create_sql))
        .await?;

    // Close the postgres connection
    let _ = postgres_db.close().await;

    info!("✅ Database recreated successfully");

    // Now connect to the new database and run migrations
    info!("Running migrations...");
    let db = crate::database::setup_database_connection(&config.database).await;

    // Run all migrations up
    crate::commands::migrate::handle_migration_command::<AppMigrator>(
        &db,
        MigrateAction::Up { steps: None },
    )
    .await?;

    info!("✅ Database reset completed successfully");

    Ok(())
}
