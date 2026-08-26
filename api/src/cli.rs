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
    /// Database management commands
    Db {
        #[command(subcommand)]
        action: Option<DbAction>,
    },
    /// Generate a JWT secret for configuration
    GenerateJwtSecret,
    /// Show version information
    Version,
    /// List all application routes
    Routes,
}

#[derive(Subcommand)]
pub enum DbAction {
    /// Open a database connection with psql
    Console,
    /// Drop all tables and types, then run all migrations
    Reset,
    /// Database migration commands
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("erno").chain(args.iter().copied()))
    }

    #[test]
    fn db_defaults_to_console() {
        let cli = parse(&["db"]).expect("db should parse");
        assert!(matches!(cli.command, Some(Commands::Db { action: None })));
    }

    #[test]
    fn db_console() {
        let cli = parse(&["db", "console"]).expect("db console should parse");
        assert!(matches!(
            cli.command,
            Some(Commands::Db {
                action: Some(DbAction::Console)
            })
        ));
    }

    #[test]
    fn db_reset() {
        let cli = parse(&["db", "reset"]).expect("db reset should parse");
        assert!(matches!(
            cli.command,
            Some(Commands::Db {
                action: Some(DbAction::Reset)
            })
        ));
    }

    #[test]
    fn db_migrate_up() {
        let cli = parse(&["db", "migrate", "up"]).expect("db migrate up should parse");
        assert!(matches!(
            cli.command,
            Some(Commands::Db {
                action: Some(DbAction::Migrate {
                    action: MigrateAction::Up { steps: None }
                })
            })
        ));
    }

    #[test]
    fn db_migrate_down_steps() {
        let cli = parse(&["db", "migrate", "down", "--steps", "2"])
            .expect("db migrate down --steps 2 should parse");
        assert!(matches!(
            cli.command,
            Some(Commands::Db {
                action: Some(DbAction::Migrate {
                    action: MigrateAction::Down { steps: 2 }
                })
            })
        ));
    }

    #[test]
    fn db_migrate_status() {
        let cli = parse(&["db", "migrate", "status"]).expect("db migrate status should parse");
        assert!(matches!(
            cli.command,
            Some(Commands::Db {
                action: Some(DbAction::Migrate {
                    action: MigrateAction::Status
                })
            })
        ));
    }

    #[test]
    fn db_migrate_reset() {
        let cli = parse(&["db", "migrate", "reset"]).expect("db migrate reset should parse");
        assert!(matches!(
            cli.command,
            Some(Commands::Db {
                action: Some(DbAction::Migrate {
                    action: MigrateAction::Reset
                })
            })
        ));
    }

    #[test]
    fn db_migrate_reapply() {
        let cli = parse(&["db", "migrate", "reapply"]).expect("db migrate reapply should parse");
        assert!(matches!(
            cli.command,
            Some(Commands::Db {
                action: Some(DbAction::Migrate {
                    action: MigrateAction::Reapply { steps: 1 }
                })
            })
        ));
    }

    #[test]
    fn top_level_migrate_is_unknown() {
        assert!(parse(&["migrate"]).is_err());
        assert!(parse(&["migrate", "up"]).is_err());
    }
}
