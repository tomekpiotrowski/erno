use std::{cmp, error::Error, process};

use sea_orm::DatabaseConnection;

use crate::{
    database::setup_database_connection,
    {cli::MigrateAction, config::Config},
};

pub async fn handle_migrate_command<AppMigrator: sea_orm_migration::MigratorTrait, ExtraConfig>(
    config: &Config<ExtraConfig>,
    action: MigrateAction,
) where
    ExtraConfig: Clone,
{
    // Create a simple connection just for migrations (no background setup)
    let db = setup_database_connection(&config.database).await;

    if let Err(e) = handle_migration_command::<AppMigrator>(&db, action).await {
        eprintln!("❌ Migration failed: {e}");
        process::exit(1);
    }
}

pub async fn handle_migration_command<AppMigrator: sea_orm_migration::MigratorTrait>(
    db: &DatabaseConnection,
    action: MigrateAction,
) -> Result<(), Box<dyn Error>> {
    match action {
        MigrateAction::Up { steps } => {
            println!("Running migrations up...");

            // Get pending migrations to show what will be applied
            let pending_migrations = AppMigrator::get_pending_migrations(db).await?;

            if pending_migrations.is_empty() {
                println!("✅ All migrations are already up to date");
                return Ok(());
            }

            #[allow(clippy::option_if_let_else)] // The if-let pattern is clearer here
            let migrations_to_apply = if let Some(steps) = steps {
                let count = cmp::min(steps as usize, pending_migrations.len());
                println!("Running {count} migration(s) up:");
                &pending_migrations[..count]
            } else {
                println!(
                    "Running all {} pending migration(s) up:",
                    pending_migrations.len()
                );
                &pending_migrations[..]
            };

            // Show what will be applied
            for migration in migrations_to_apply {
                println!("  📄 {}", migration.name());
            }
            println!();

            // Apply migrations
            match steps {
                Some(steps) => {
                    AppMigrator::up(db, Some(steps)).await?;
                }
                None => {
                    AppMigrator::up(db, None).await?;
                }
            }

            println!("✅ Migrations completed successfully");
        }
        MigrateAction::Down { steps } => {
            println!("Rolling back {steps} migration(s)...");

            // Get applied migrations to show what will be reverted
            let applied_migrations = AppMigrator::get_applied_migrations(db).await?;

            if applied_migrations.is_empty() {
                println!("❌ No migrations to roll back");
                return Ok(());
            }

            let migrations_to_revert = cmp::min(steps as usize, applied_migrations.len());
            let revert_slice =
                &applied_migrations[applied_migrations.len() - migrations_to_revert..];

            println!("Rolling back migrations:");
            for migration in revert_slice.iter().rev() {
                println!("  📄 {}", migration.name());
            }
            println!();

            AppMigrator::down(db, Some(steps)).await?;
            println!("✅ Rollback completed successfully");
        }
        MigrateAction::Status => {
            match AppMigrator::get_pending_migrations(db).await {
                Ok(pending) => {
                    if pending.is_empty() {
                        println!("✅ All migrations are up to date");
                    } else {
                        println!("📋 Pending migrations:");
                        for migration in pending {
                            println!("  - {}", migration.name());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Failed to check migration status: {e}");
                    return Err(e.into());
                }
            }

            match AppMigrator::get_applied_migrations(db).await {
                Ok(applied) => {
                    println!("📋 Applied migrations:");
                    for migration in applied {
                        println!("  ✓ {}", migration.name());
                    }
                }
                Err(e) => {
                    eprintln!("❌ Failed to get applied migrations: {e}");
                    return Err(e.into());
                }
            }
        }
        MigrateAction::Reset => {
            println!("🔄 Resetting database (this will drop all data!)...");

            // First, get all applied migrations to know how many to roll back
            let applied = AppMigrator::get_applied_migrations(db).await?;
            let num_applied =
                u32::try_from(applied.len()).map_err(|_| "Too many migrations to reset")?;

            if num_applied > 0 {
                println!("Rolling back {num_applied} applied migrations:");
                for migration in applied.iter().rev() {
                    println!("  📄 {}", migration.name());
                }
                println!();

                AppMigrator::down(db, Some(num_applied)).await?;
                println!("✅ All migrations rolled back");
            } else {
                println!("No migrations to roll back");
            }

            // Get all available migrations for applying up
            let pending = AppMigrator::get_pending_migrations(db).await?;
            println!("Running all {} migration(s) up:", pending.len());
            for migration in &pending {
                println!("  📄 {}", migration.name());
            }
            println!();

            AppMigrator::up(db, None).await?;
            println!("✅ Database reset completed successfully");
        }
        MigrateAction::Reapply { steps } => {
            // Get applied migrations to check what we can reapply
            let applied_migrations = AppMigrator::get_applied_migrations(db).await?;

            if applied_migrations.is_empty() {
                println!("❌ No migrations to reapply");
                return Ok(());
            }

            let migrations_to_reapply = cmp::min(steps as usize, applied_migrations.len());
            let reapply_slice =
                &applied_migrations[applied_migrations.len() - migrations_to_reapply..];

            println!("🔄 Reapplying {migrations_to_reapply} migration(s):");
            for migration in reapply_slice.iter().rev() {
                println!("  📄 {}", migration.name());
            }
            println!();

            // Step 1: Roll back the specified migrations
            println!("Step 1: Rolling back {migrations_to_reapply} migration(s)...");
            AppMigrator::down(db, Some(steps)).await?;
            println!("✅ Rollback completed");

            // Step 2: Reapply the same migrations
            println!("Step 2: Reapplying {migrations_to_reapply} migration(s)...");
            AppMigrator::up(db, Some(steps)).await?;
            println!("✅ Reapply completed successfully");
        }
    }

    Ok(())
}
