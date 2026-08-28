use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{routing::get, Router};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, Tokio1Executor};
use sea_orm_migration::MigratorTrait;
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::{
    api::health_checks::ok,
    app::App,
    auth::jwt::validate_jwt_secret,
    config::Config,
    database::setup_database,
    environment::Environment,
    jobs::{
        failure_handler::JobFailureHandler, job_registry::JobRegistry,
        job_supervisor::job_supervisor, scheduled_job::ScheduledJob,
    },
    metrics,
    router::router,
    sync::registry::SyncRegistry,
    websocket::connections::Connections,
};

// Boot wiring: every argument is a distinct subsystem the server needs.
#[allow(clippy::too_many_arguments)]
pub async fn handle_serve_command<AppMigrator: MigratorTrait + 'static, ExtraConfig>(
    environment: Environment,
    config: Config<ExtraConfig>,
    app_router: fn(App<ExtraConfig>) -> Router,
    job_registry: JobRegistry<ExtraConfig>,
    job_schedule: Vec<ScheduledJob>,
    sync_registry: SyncRegistry,
    job_failure_handler: Option<Arc<dyn JobFailureHandler>>,
    user_data_deleter: Option<Arc<dyn crate::account::UserDataDeleter>>,
    metrics_collectors: crate::metrics::collector::CollectorRegistry,
    app_info: crate::app_info::AppInfo,
    skip_default_cors: bool,
) where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let port = config.server.port;

    // Validate JWT secret strength before starting.
    if let Err(msg) = validate_jwt_secret(&config) {
        if environment == Environment::Production {
            panic!("🔐 {msg} Refusing to start in production with a weak JWT secret.");
        } else {
            tracing::warn!("🔐 {msg}");
        }
    }

    // We start a temporary liveness server for Kubernetes to know that the application is alive
    let liveness_server_task = tokio::spawn(start_liveness_server(port));

    let (db, migration_receiver) = setup_database::<AppMigrator>(&config.database).await;

    // Wait for migrations to complete
    match migration_receiver.await {
        Ok(Ok(())) => {
            info!("✅ Database is ready!");
        }
        Ok(Err(e)) => {
            error!("❌ Database setup failed: {}", e);
            liveness_server_task.abort();
            return;
        }
        Err(_) => {
            error!("❌ Database setup channel closed unexpectedly");
            liveness_server_task.abort();
            return;
        }
    }

    let mailer = match &config.email {
        crate::config::EmailConfig::Mock => crate::mailer::Mailer::mock(),
        crate::config::EmailConfig::Smtp {
            host,
            port,
            username,
            password,
            use_tls,
            ..
        } => {
            let mut mailer_builder = if *use_tls {
                AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                    .expect("Failed to create mailer transport")
                    .port(*port)
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host).port(*port)
            };

            if let (Some(username), Some(password)) = (username, password) {
                mailer_builder = mailer_builder
                    .credentials(Credentials::new(username.clone(), password.clone()));
            }

            crate::mailer::Mailer::smtp(mailer_builder.build())
        }
    };

    let job_queue = crate::job_queue::JobQueue::database();
    let sync_queue = crate::sync::queue::SyncQueue::database();
    let sync_registry = Arc::new(sync_registry);

    // Initialize rate limiting state
    let rate_limit_state = crate::rate_limiting::RateLimitState::new(config.rate_limiting.clone());

    // Periodically clean up stale IP entries to prevent unbounded memory growth
    {
        let cleanup_state = rate_limit_state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                cleanup_state.cleanup_expired_entries();
            }
        });
    }

    // Initialize WebSocket connections manager
    let websocket_connections = Connections::new();

    let storage = crate::storage::FileStorage::from_config(&config.storage);

    // Set up Prometheus metrics recorder
    let prometheus_handle = metrics::setup_metrics();
    let metrics_collectors = Arc::new(metrics_collectors);

    // Error reporting. The reporter is built before the App so the capture
    // hooks can be armed immediately: a panic during the rest of boot is
    // exactly the kind of failure worth catching.
    let error_reporting = Arc::new(config.error_reporting.clone());
    // Installed before anything long-running starts, so a signal arriving
    // during boot is not missed.
    let (shutdown_signal, shutdown) = crate::shutdown::listen();
    let (error_reporter, reporter_task) =
        crate::error_reporting::reporter::ErrorReporter::start_with_shutdown(
            &error_reporting,
            app_info,
            environment,
            shutdown.clone(),
        );
    if error_reporter.is_active() {
        crate::error_reporting::reporter::capture::install(
            error_reporter.clone(),
            Arc::clone(&error_reporting),
        );
        if error_reporting.capture_panics {
            // Catches panics outside any request: job workers, the sync and
            // websocket listeners, background loops.
            crate::error_reporting::reporter::capture::install_panic_hook();
        }
    }

    // Subsystem health. Independent of error capture — a deployment may want
    // liveness without error reporting, or the reverse.
    if error_reporting.is_active() && error_reporting.report_health {
        crate::health::spawn_health_reporter(
            db.clone(),
            websocket_connections.clone(),
            crate::health::HealthReporterConfig {
                endpoint: error_reporting.health_endpoint(),
                token: error_reporting.ingest_token.clone(),
                interval: Duration::from_secs(error_reporting.health_interval_seconds.max(5)),
                request_timeout: Duration::from_millis(error_reporting.request_timeout_ms.max(1)),
                // Distinguishes replicas. The hostname is what a container
                // orchestrator sets, and is what an operator recognises.
                instance: std::env::var("HOSTNAME")
                    .ok()
                    .filter(|h| !h.is_empty())
                    .unwrap_or_else(|| format!("{}-local", app_info.name)),
                release: Some(app_info.version.to_string()),
                environment: environment.to_string(),
                job_timeout_seconds: config.jobs.defaults.job_timeout,
            },
        );
    }

    crate::dev::migrations::install::<AppMigrator>();

    let app = App {
        config: config.clone(),
        environment,
        db: db.clone(),
        mailer,
        job_queue,
        sync_queue,
        sync_registry: sync_registry.clone(),
        rate_limit_state,
        websocket_connections: websocket_connections.clone(),
        storage,
        metrics_collectors: metrics_collectors.clone(),
        prometheus_handle,
        job_failure_handler,
        user_data_deleter,
        error_reporter,
        skip_default_cors,
    };

    // Spawn workers in the background. The handle is retained: this is the one
    // task whose abrupt death corrupts state, by leaving job rows in `running`.
    let jobs_task = tokio::spawn(job_supervisor(
        config.jobs,
        app.clone(),
        job_registry,
        job_schedule,
        shutdown.clone(),
    ));

    // Spawn WebSocket listener in the background
    let listener_db = db.clone();
    let listener_connections = websocket_connections.clone();
    tokio::spawn(async move {
        crate::websocket::listener::start_listener(listener_db, listener_connections).await;
    });

    // Spawn sync push listener in the background
    let sync_listener_db = db.clone();
    let sync_listener_connections = websocket_connections.clone();
    let sync_listener_registry = sync_registry.clone();
    tokio::spawn(async move {
        crate::sync::listener::start_sync_listener(
            sync_listener_db,
            sync_listener_connections,
            sync_listener_registry,
        )
        .await;
    });

    // Spawn DB stats + custom metrics collector task
    if config.metrics.enabled {
        let stats_db = db.clone();
        let stats_config = config.metrics.clone();
        let stats_collectors = (*metrics_collectors).clone();
        tokio::spawn(async move {
            metrics::db_stats::db_stats_task(stats_db, stats_config, stats_collectors).await;
        });
    }

    // Stop the temporary liveness server
    liveness_server_task.abort();
    let _ = liveness_server_task.await;

    // Start the full server
    let router = router(app.clone(), app_router);
    start_server(router, port, shutdown).await;

    // Past this point the listener is closed and in-flight requests have
    // finished. Now let the parts that hold state finish tidying up.
    shutdown_signal.trigger();

    // Workers first: a job left in `running` is invisible until the stuck-job
    // sweeper reclaims it, which is the only shutdown failure that outlives the
    // process.
    match tokio::time::timeout(crate::shutdown::DRAIN_TIMEOUT, jobs_task).await {
        Ok(_) => info!("✅ Job workers stopped cleanly"),
        Err(_) => tracing::warn!(
            "Job workers did not stop within the drain timeout; \
             any job still running will be reclaimed by the stuck-job sweeper"
        ),
    }

    // Then the error reporter, so the errors that caused the shutdown are
    // actually reported rather than dying with the buffer.
    if let Some(task) = reporter_task {
        match tokio::time::timeout(Duration::from_secs(5), task).await {
            Ok(_) => info!("✅ Error reports flushed"),
            Err(_) => tracing::warn!("Error reporter did not flush in time; reports were dropped"),
        }
    }

    // Outstanding spans last: anything still in the batch exporter after the
    // listener and workers have stopped.
    crate::tracing_otel::shutdown();

    info!("👋 Shutdown complete");
}

// Minimal server that only serves liveness endpoint during migrations
async fn start_liveness_server(port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();

    let migration_router = Router::new().route("/liveness", get(ok));
    axum::serve(listener, migration_router).await.unwrap();
}

// Full server with all endpoints
async fn start_server(router: Router, port: u16, mut shutdown: crate::shutdown::Shutdown) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();

    info!("🌐 Server starting on http://{}", addr);
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    // Stops accepting connections on SIGTERM and waits for in-flight requests.
    // Bounded, because a long-poll or a websocket would otherwise hold the pod
    // open until Kubernetes loses patience and SIGKILLs it mid-drain.
    .with_graceful_shutdown(async move {
        shutdown.recv().await;
        info!("🚪 No longer accepting connections; finishing in-flight requests");
    })
    .await
    .unwrap();
}
