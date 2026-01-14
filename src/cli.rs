use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(about = env!("CARGO_PKG_DESCRIPTION"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the web server (default)
    Serve,
    /// Database migration commands
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Database management commands
    Db {
        #[command(subcommand)]
        action: Option<DbAction>,
    },
    /// Interactive Rhai console
    Console,
    /// Generate a JWT secret for configuration
    GenerateJwtSecret,
    /// Show version information
    Version,
}

#[derive(Subcommand)]
pub enum DbAction {
    /// Open a database connection with psql
    Console,
    /// Drop and recreate the database, then run all migrations
    Reset,
}

#[derive(Subcommand)]
pub enum MigrateAction {
    /// Run migrations up
    Up {
        /// Number of migrations to run (default: all)
        #[arg(short, long)]
        steps: Option<u32>,
    },
    /// Run migrations down
    Down {
        /// Number of migrations to rollback (default: 1)
        #[arg(short, long, default_value = "1")]
        steps: u32,
    },
    /// Show migration status
    Status,
    /// Reset database (down all, then up all)
    Reset,
    /// Reapply recent migrations (down then up)
    Reapply {
        /// Number of migrations to reapply (default: 1)
        #[arg(short, long, default_value = "1")]
        steps: u32,
    },
}
