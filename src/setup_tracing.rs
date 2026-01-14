use time::format_description::parse;
use tracing_subscriber::fmt::time::OffsetTime;

use crate::cli::Commands;

pub fn setup_tracing_for_command(command: &Option<Commands>, server_log_level: &str) {
    // Set appropriate default tracing level based on command type:
    // - CLI commands (migrate, console, version) use 'warn'/'error' to reduce noise
    // - Server mode uses 'info' for operational visibility
    // - Users can override with RUST_LOG environment variable (e.g., RUST_LOG=debug)
    let default_level = match command {
        // CLI commands should have minimal log output for clean UX
        Some(Commands::Migrate { .. } | Commands::Db { .. } | Commands::Console) => "warn",
        Some(Commands::Version | Commands::GenerateJwtSecret | Commands::Routes) => "error", // Version, GenerateJwtSecret, and Routes should be very quiet
        // Server mode needs operational visibility
        Some(Commands::Serve) | None => server_log_level,
    };

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level))
        // Filter out noisy third-party logs
        .add_directive("sqlx::postgres::notice=warn".parse().unwrap())
        .add_directive("sea_orm_migration::migrator=warn".parse().unwrap());

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false) // Remove module paths for cleaner output
        .with_thread_ids(false) // Remove thread IDs for cleaner output
        .with_thread_names(false) // Remove thread names for cleaner output
        .with_level(true)
        .with_ansi(true) // Enable colors
        .with_timer(OffsetTime::new(
            time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
            parse("[hour]:[minute]:[second].[subsecond digits:2]").unwrap(),
        ))
        .compact() // Use compact format
        .init();
}
