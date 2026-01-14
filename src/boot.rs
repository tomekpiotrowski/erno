use std::{env, str::FromStr as _};

use axum::Router;
use clap::Parser as _;
use config_rs::Config as ConfigRs;
use sea_orm_migration::MigratorTrait;
use tracing::{debug, trace};

use crate::{
    app::App,
    app_info::AppInfo,
    cli::{Cli, Commands},
    commands::{console, db, db_reset, migrate, serve, version},
    config::Config,
    environment::Environment,
    jobs::{job_registry::JobRegistry, scheduled_job::ScheduledJob},
    setup_tracing::setup_tracing_for_command,
};

const ENVIRONMENT_VARIABLE: &str = "APP_ENVIRONMENT";

/// Configuration for bootstrapping the application.
///
/// Contains all the necessary components to start the application,
/// including metadata, routing, job processing, and scheduling.
pub struct BootConfig {
    pub app_info: AppInfo,
    pub app_router: fn(App) -> Router,
    pub job_registry: JobRegistry,
    pub job_schedule: Vec<ScheduledJob>,
}

impl BootConfig {
    #[must_use]
    pub const fn new(
        app_info: AppInfo,
        app_router: fn(App) -> Router,
        job_registry: JobRegistry,
        job_schedule: Vec<ScheduledJob>,
    ) -> Self {
        Self {
            app_info,
            app_router,
            job_registry,
            job_schedule,
        }
    }
}

pub async fn boot<AppMigrator: MigratorTrait>(config: BootConfig) {
    let cli = Cli::parse();

    if matches!(cli.command, Some(Commands::Version)) {
        version::print_version_info(config.app_info);
        return;
    }

    let environment = set_environment();

    let app_config = read_config(&environment);

    // Set up tracing with appropriate level based on command
    setup_tracing_for_command(&cli.command, &app_config.tracing.log_level);

    debug!("Environment set to: {:?}", environment);
    trace!("Configuration loaded: {:?}", app_config);

    handle_command::<AppMigrator>(
        environment,
        app_config,
        cli,
        config.app_router,
        config.job_registry,
        config.job_schedule,
        config.app_info,
    )
    .await;
}

#[must_use]
pub fn set_environment() -> Environment {
    env::var(ENVIRONMENT_VARIABLE)
        .ok()
        .and_then(|s| Environment::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn read_config(environment: &Environment) -> Config {
    let config_file_name = format!("config/{environment}");

    trace!("Reading configuration from: {}", config_file_name);

    ConfigRs::builder()
        .add_source(config_rs::File::with_name(&config_file_name))
        .add_source(config_rs::Environment::with_prefix("APP"))
        .build()
        .unwrap()
        .try_deserialize()
        .expect("Failed to deserialize configuration")
}

pub async fn handle_command<AppMigrator: MigratorTrait>(
    environment: Environment,
    config: Config,
    cli: Cli,
    app_router: fn(App) -> Router,
    job_registry: JobRegistry,
    job_schedule: Vec<ScheduledJob>,
    app_info: AppInfo,
) {
    match cli.command {
        Some(Commands::Migrate { action }) => {
            migrate::handle_migrate_command::<AppMigrator>(&config, action).await;
        }
        Some(Commands::Db { action }) => match action {
            Some(crate::cli::DbAction::Console) | None => {
                db::handle_db_console_command(&config);
            }
            Some(crate::cli::DbAction::Reset) => {
                db_reset::handle_db_reset_command::<AppMigrator>(&config).await;
            }
        },
        Some(Commands::Console) => {
            console::handle_console_command(environment);
        }
        Some(Commands::GenerateJwtSecret) => {
            crate::commands::generate_secret::handle_generate_secret_command();
        }
        Some(Commands::Version) => {
            version::print_version_info(app_info);
        }
        Some(Commands::Serve) | None => {
            serve::handle_serve_command::<AppMigrator>(
                environment,
                config,
                app_router,
                job_registry,
                job_schedule,
            )
            .await;
        }
    }
}
