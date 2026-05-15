use std::{env, str::FromStr as _};

use axum::Router;
use clap::Parser as _;
use config_rs::Config as ConfigRs;
use sea_orm_migration::MigratorTrait;
use serde::de::DeserializeOwned;
use tracing::{debug, trace};

use crate::{
    app::App,
    app_info::AppInfo,
    cli::{Cli, Commands},
    commands::{db, db_reset, migrate, routes, serve, version},
    config::Config,
    environment::Environment,
    jobs::{
        job_registry::JobRegistry,
        scheduled_job::ScheduledJob,
        send_already_registered_email_job::SendAlreadyRegisteredEmailJob,
        send_password_reset_email_job::SendPasswordResetEmailJob,
        send_verification_email_job::SendVerificationEmailJob,
    },
    setup_tracing::setup_tracing_for_command,
    sync::registry::SyncRegistry,
};

const ENVIRONMENT_VARIABLE: &str = "APP_ENVIRONMENT";

/// Configuration for bootstrapping the application.
///
/// Contains all the necessary components to start the application,
/// including metadata, routing, job processing, and scheduling.
pub struct BootConfig<ExtraConfig = ()> {
    pub app_info: AppInfo,
    pub app_router: fn(App<ExtraConfig>) -> Router,
    pub job_registry: JobRegistry<ExtraConfig>,
    pub job_schedule: Vec<ScheduledJob>,
    pub sync_registry: SyncRegistry,
}

impl<ExtraConfig> BootConfig<ExtraConfig> {
    #[must_use]
    pub fn new(
        app_info: AppInfo,
        app_router: fn(App<ExtraConfig>) -> Router,
        job_registry: JobRegistry<ExtraConfig>,
        job_schedule: Vec<ScheduledJob>,
    ) -> Self {
        Self {
            app_info,
            app_router,
            job_registry,
            job_schedule,
            sync_registry: SyncRegistry::new(),
        }
    }

    /// Register a syncable entity in the sync registry.
    #[must_use]
    pub fn with_sync<E>(mut self) -> Self
    where
        E: crate::sync::syncable::Syncable,
        E::Model: serde::de::DeserializeOwned,
    {
        self.sync_registry = self.sync_registry.register::<E>();
        self
    }
}

pub async fn boot<AppMigrator: MigratorTrait, ExtraConfig>(config: BootConfig<ExtraConfig>)
where
    ExtraConfig: Clone + Default + DeserializeOwned + Send + Sync + 'static,
{
    let cli = Cli::parse();

    if matches!(cli.command, Some(Commands::Version)) {
        version::print_version_info(config.app_info);
        return;
    }

    let environment = set_environment();

    let app_config = read_config::<ExtraConfig>(&environment);

    // Set up tracing with appropriate level based on command
    setup_tracing_for_command(&cli.command, &app_config.tracing.log_level);

    debug!("Environment set to: {:?}", environment);
    trace!("Configuration loaded");

    let mut job_registry = config.job_registry;
    register_builtin_jobs::<ExtraConfig>(&mut job_registry);

    handle_command::<AppMigrator, ExtraConfig>(
        environment,
        app_config,
        cli,
        config.app_router,
        job_registry,
        config.job_schedule,
        config.sync_registry,
        config.app_info,
    )
    .await;
}

fn register_builtin_jobs<ExtraConfig>(job_registry: &mut JobRegistry<ExtraConfig>)
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    job_registry.register_job::<SendVerificationEmailJob<ExtraConfig>>();
    job_registry.register_job::<SendPasswordResetEmailJob<ExtraConfig>>();
    job_registry.register_job::<SendAlreadyRegisteredEmailJob<ExtraConfig>>();
}

#[must_use]
pub fn set_environment() -> Environment {
    env::var(ENVIRONMENT_VARIABLE)
        .ok()
        .and_then(|s| Environment::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn read_config<ExtraConfig>(environment: &Environment) -> Config<ExtraConfig>
where
    ExtraConfig: Default + DeserializeOwned,
{
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

pub async fn handle_command<AppMigrator: MigratorTrait, ExtraConfig>(
    environment: Environment,
    config: Config<ExtraConfig>,
    cli: Cli,
    app_router: fn(App<ExtraConfig>) -> Router,
    job_registry: JobRegistry<ExtraConfig>,
    job_schedule: Vec<ScheduledJob>,
    sync_registry: SyncRegistry,
    app_info: AppInfo,
) where
    ExtraConfig: Clone + Default + DeserializeOwned + Send + Sync + 'static,
{
    match cli.command {
        Some(Commands::Migrate { action }) => {
            migrate::handle_migrate_command::<AppMigrator, ExtraConfig>(&config, action).await;
        }
        Some(Commands::Db { action }) => match action {
            Some(crate::cli::DbAction::Console) | None => {
                db::handle_db_console_command(&config);
            }
            Some(crate::cli::DbAction::Reset) => {
                db_reset::handle_db_reset_command::<AppMigrator, ExtraConfig>(&config).await;
            }
        },
        Some(Commands::GenerateJwtSecret) => {
            crate::commands::generate_secret::handle_generate_secret_command();
        }
        Some(Commands::Version) => {
            version::print_version_info(app_info);
        }
        Some(Commands::Routes) => {
            routes::handle_routes_command::<ExtraConfig>(config, app_router).await;
        }
        #[cfg(feature = "admin")]
        Some(Commands::Admin) => {
            let db = crate::database::setup_database_connection(&config.database).await;
            crate::commands::admin::handle_admin_command(db, config.stripe).await;
        }
        Some(Commands::Serve) | None => {
            serve::handle_serve_command::<AppMigrator, ExtraConfig>(
                environment,
                config,
                app_router,
                job_registry,
                job_schedule,
                sync_registry,
            )
            .await;
        }
    }
}
