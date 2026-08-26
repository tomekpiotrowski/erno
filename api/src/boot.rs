use std::{env, str::FromStr as _};

use axum::Router;
use clap::Parser as _;
use config_rs::Config as ConfigRs;
use sea_orm_migration::MigratorTrait;
use serde::de::DeserializeOwned;
use tracing::{debug, trace};

use crate::{
    account::UserDataDeleter,
    app::App,
    app_info::AppInfo,
    billing::jobs::{
        cancel_stripe_subscription_job::CancelStripeSubscriptionJob,
        delete_stripe_customer_job::DeleteStripeCustomerJob,
    },
    cli::{Cli, Commands},
    commands::{db, db_reset, migrate, routes, serve, version},
    config::Config,
    environment::Environment,
    jobs::{
        failure_handler::JobFailureHandler, job_registry::JobRegistry, scheduled_job::ScheduledJob,
        send_already_registered_email_job::SendAlreadyRegisteredEmailJob,
        send_password_reset_email_job::SendPasswordResetEmailJob,
        send_verification_email_job::SendVerificationEmailJob,
    },
    setup_tracing::setup_tracing_for_command,
    storage::delete_user_files_job::DeleteUserFilesJob,
    sync::registry::SyncRegistry,
};
use std::sync::Arc;

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
    pub job_failure_handler: Option<Arc<dyn JobFailureHandler>>,
    pub user_data_deleter: Option<Arc<dyn UserDataDeleter>>,
    pub metrics_collectors: crate::metrics::collector::CollectorRegistry,
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
            job_failure_handler: None,
            user_data_deleter: None,
            metrics_collectors: crate::metrics::collector::CollectorRegistry::default(),
        }
    }

    /// Replace the sync registry (tests that build a registry by hand).
    #[must_use]
    pub fn with_sync_registry(mut self, sync_registry: SyncRegistry) -> Self {
        self.sync_registry = sync_registry;
        self
    }

    /// Register a syncable entity in the sync registry.
    #[must_use]
    pub fn with_sync<E>(mut self) -> Self
    where
        E: crate::sync::syncable::Syncable,
        E::Policy: crate::sync::from_user::FromUser,
        E::Model: serde::de::DeserializeOwned,
    {
        self.sync_registry = self.sync_registry.register::<E>();
        self
    }

    /// Register a shareable syncable entity in the sync registry.
    ///
    /// The entity's policy must implement `FromPrincipal`; active shares held
    /// by a request or connection then widen read access, and shares may be
    /// created for this entity type via the share endpoints.
    #[must_use]
    pub fn with_sync_shared<E>(mut self) -> Self
    where
        E: crate::sync::syncable::Syncable,
        E::Policy: crate::share::principal::FromPrincipal,
        E::Model: serde::de::DeserializeOwned,
        <E::PrimaryKey as sea_orm::PrimaryKeyTrait>::ValueType: From<uuid::Uuid>,
    {
        self.sync_registry = self.sync_registry.register_shareable::<E>();
        self
    }

    /// Register an app-wide handler invoked whenever any job permanently fails.
    #[must_use]
    pub fn on_job_failure(mut self, handler: Arc<dyn JobFailureHandler>) -> Self {
        self.job_failure_handler = Some(handler);
        self
    }

    /// Register a hook that deletes app-owned per-user data on account deletion.
    #[must_use]
    pub fn on_delete_user(mut self, deleter: Arc<dyn UserDataDeleter>) -> Self {
        self.user_data_deleter = Some(deleter);
        self
    }

    /// Register a periodic Prometheus collector (must not `COUNT(*)` large tables).
    #[must_use]
    pub fn with_metrics_collector<C>(mut self, collector: C) -> Self
    where
        C: crate::metrics::collector::MetricsCollector + 'static,
    {
        self.metrics_collectors.add(collector);
        self
    }
}

pub async fn boot<AppMigrator: MigratorTrait + 'static, ExtraConfig>(
    config: BootConfig<ExtraConfig>,
) where
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
    setup_tracing_for_command(&cli.command, &app_config.tracing);

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
        config.job_failure_handler,
        config.user_data_deleter,
        config.metrics_collectors,
    )
    .await;
}

pub(crate) fn register_builtin_jobs<ExtraConfig>(job_registry: &mut JobRegistry<ExtraConfig>)
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    job_registry.register_job::<SendVerificationEmailJob<ExtraConfig>>();
    job_registry.register_job::<SendPasswordResetEmailJob<ExtraConfig>>();
    job_registry.register_job::<SendAlreadyRegisteredEmailJob<ExtraConfig>>();
    // Account-deletion cleanup jobs.
    job_registry.register_job::<CancelStripeSubscriptionJob<ExtraConfig>>();
    job_registry.register_job::<DeleteStripeCustomerJob<ExtraConfig>>();
    job_registry.register_job::<DeleteUserFilesJob<ExtraConfig>>();
    job_registry
        .register_job::<crate::storage::delete_record_attachments_job::DeleteRecordAttachmentsJob<ExtraConfig>>();
    job_registry
        .register_job::<crate::error_reporting::anonymize_user_job::AnonymizeCollectorEventsJob<ExtraConfig>>();
}

/// Job types the framework registers on every app's behalf.
///
/// Derived from [`register_builtin_jobs`] rather than listed by hand, so the
/// two cannot drift — adding a built-in automatically makes it known here.
///
/// Used by the worker-coverage check: an app author cannot be expected to have
/// listed a job type that did not exist when they wrote their config.
pub(crate) fn builtin_job_names() -> std::collections::HashSet<&'static str> {
    let mut registry = JobRegistry::<()>::new();
    register_builtin_jobs(&mut registry);
    registry.job_names().copied().collect()
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
        .add_source(
            config_rs::Environment::with_prefix("APP")
                .separator("__")
                .try_parsing(true),
        )
        .build()
        .unwrap()
        .try_deserialize()
        .expect("Failed to deserialize configuration")
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_command<AppMigrator: MigratorTrait + 'static, ExtraConfig>(
    environment: Environment,
    config: Config<ExtraConfig>,
    cli: Cli,
    app_router: fn(App<ExtraConfig>) -> Router,
    job_registry: JobRegistry<ExtraConfig>,
    job_schedule: Vec<ScheduledJob>,
    sync_registry: SyncRegistry,
    app_info: AppInfo,
    job_failure_handler: Option<Arc<dyn JobFailureHandler>>,
    user_data_deleter: Option<Arc<dyn UserDataDeleter>>,
    metrics_collectors: crate::metrics::collector::CollectorRegistry,
) where
    ExtraConfig: Clone + Default + DeserializeOwned + Send + Sync + 'static,
{
    match cli.command {
        Some(Commands::Db { action }) => match action {
            Some(crate::cli::DbAction::Console) | None => {
                db::handle_db_console_command(&config);
            }
            Some(crate::cli::DbAction::Reset) => {
                db_reset::handle_db_reset_command::<AppMigrator, ExtraConfig>(&config).await;
            }
            Some(crate::cli::DbAction::Migrate { action }) => {
                migrate::handle_migrate_command::<AppMigrator, ExtraConfig>(&config, action).await;
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
        Some(Commands::Serve) | None => {
            serve::handle_serve_command::<AppMigrator, ExtraConfig>(
                environment,
                config,
                app_router,
                job_registry,
                job_schedule,
                sync_registry,
                job_failure_handler,
                user_data_deleter,
                metrics_collectors,
                app_info,
            )
            .await;
        }
    }
}
