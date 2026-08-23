use time::format_description::parse_borrowed;
use tracing_subscriber::{fmt::time::OffsetTime, layer::SubscriberExt, util::SubscriberInitExt};

use crate::cli::Commands;

pub fn setup_tracing_for_command(command: &Option<Commands>, server_log_level: &str) {
    // Set appropriate default tracing level based on command type:
    // - CLI commands (db, version) use 'warn'/'error' to reduce noise
    // - Server mode uses 'info' for operational visibility
    // - Users can override with RUST_LOG environment variable (e.g., RUST_LOG=debug)
    let default_level = match command {
        // CLI commands should have minimal log output for clean UX
        Some(Commands::Db { .. }) => "warn",
        Some(Commands::Version | Commands::GenerateJwtSecret | Commands::Routes) => "error", // Version, GenerateJwtSecret, and Routes should be very quiet
        // Server mode needs operational visibility
        Some(Commands::Serve) | None => server_log_level,
    };

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level))
        // Filter out noisy third-party logs
        .add_directive("sqlx::postgres::notice=warn".parse().unwrap())
        .add_directive("sea_orm_migration::migrator=warn".parse().unwrap());

    // Composed as a layered registry rather than the `fmt()` builder so error
    // reporting can observe events. Every formatting option below is carried
    // over from the builder one-for-one — operators read these logs, so the
    // output must be byte-identical.
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(false) // Remove module paths for cleaner output
        .with_thread_ids(false) // Remove thread IDs for cleaner output
        .with_thread_names(false) // Remove thread names for cleaner output
        .with_level(true)
        .with_ansi(true) // Enable colors
        .with_timer(OffsetTime::new(
            time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
            parse_borrowed::<2>("[hour]:[minute]:[second].[subsecond digits:2]").unwrap(),
        ))
        .compact(); // Use compact format

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        // Inert until the application installs a reporter; see
        // `error_reporting::reporter::capture`.
        .with(crate::error_reporting::reporter::capture::ErrorCaptureLayer)
        .init();
}
