use time::format_description::parse_borrowed;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::{
    fmt::time::OffsetTime, layer::SubscriberExt, util::SubscriberInitExt, Layer,
};

use crate::cli::Commands;
use crate::config::TracingConfig;

pub fn setup_tracing_for_command(command: &Option<Commands>, tracing: &TracingConfig) {
    // Set appropriate default tracing level based on command type:
    // - CLI commands (db, version) use 'warn'/'error' to reduce noise
    // - Server mode uses 'info' for operational visibility
    // - Users can override with RUST_LOG environment variable (e.g., RUST_LOG=debug)
    let is_server = matches!(command, Some(Commands::Serve) | None);
    let default_level = match command {
        // CLI commands should have minimal log output for clean UX
        Some(Commands::Db { .. }) => "warn",
        Some(Commands::Version | Commands::GenerateJwtSecret | Commands::Routes) => "error", // Version, GenerateJwtSecret, and Routes should be very quiet
        // Server mode needs operational visibility
        Some(Commands::Serve) | None => tracing.log_level.as_str(),
    };

    let stdout_filter = tracing_subscriber::EnvFilter::try_from_default_env()
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
        .compact()
        .with_filter(stdout_filter);

    // The capture layer only reports ERROR events, but it must still see them
    // when the fmt layer is filtered to `warn` on CLI subcommands — so it has
    // its own filter rather than sitting behind the stdout EnvFilter.
    let capture_layer = crate::error_reporting::reporter::capture::ErrorCaptureLayer
        .with_filter(filter_fn(|meta| *meta.level() == tracing::Level::ERROR));

    // sqlx query events (with elapsed time) attach to the current span without
    // flooding stdout, which keeps its own quieter filter.
    let otel_filter = tracing_subscriber::EnvFilter::new(default_level)
        .add_directive("sqlx::query=debug".parse().unwrap())
        .add_directive("sqlx::postgres::notice=warn".parse().unwrap())
        .add_directive("sea_orm_migration::migrator=warn".parse().unwrap());

    let otel_layer = is_server
        .then(|| crate::tracing_otel::trace_layer(&tracing.otel))
        .flatten()
        .map(|layer| layer.with_filter(otel_filter));

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(capture_layer)
        .with(otel_layer)
        .init();
}
