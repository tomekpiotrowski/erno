use std::net::SocketAddr;

use axum::{routing::get, Router};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, Tokio1Executor};
use sea_orm_migration::MigratorTrait;
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::{
    api::health_checks::ok,
    app::App,
    config::Config,
    database::setup_database,
    environment::Environment,
    jobs::{
        job_registry::JobRegistry, job_supervisor::job_supervisor, scheduled_job::ScheduledJob,
    },
    router::router,
    websocket::connections::Connections,
};

pub async fn handle_serve_command<AppMigrator: MigratorTrait>(
    environment: Environment,
    config: Config,
    app_router: fn(App) -> Router,
    job_registry: JobRegistry,
    job_schedule: Vec<ScheduledJob>,
) {
    let port = config.server.port;

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

    // Initialize rate limiting state
    let rate_limit_state = crate::rate_limiting::RateLimitState::new(config.rate_limiting.clone());

    // Initialize WebSocket connections manager
    let websocket_connections = Connections::new();

    let app = App {
        config: config.clone(),
        environment,
        db: db.clone(),
        mailer,
        job_queue,
        rate_limit_state,
        websocket_connections: websocket_connections.clone(),
    };

    // Spawn workers in the background
    tokio::spawn(job_supervisor(
        config.jobs,
        app.clone(),
        job_registry,
        job_schedule,
    ));

    // Spawn WebSocket listener in the background
    let listener_db = db.clone();
    let listener_connections = websocket_connections.clone();
    tokio::spawn(async move {
        crate::websocket::listener::start_listener(listener_db, listener_connections).await;
    });

    // Stop the temporary liveness server
    liveness_server_task.abort();
    let _ = liveness_server_task.await;

    // Start the full server
    let router = router(app, app_router);
    start_server(router, port).await;
}

// Minimal server that only serves liveness endpoint during migrations
async fn start_liveness_server(port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();

    let migration_router = Router::new().route("/liveness", get(ok));
    axum::serve(listener, migration_router).await.unwrap();
}

// Full server with all endpoints
async fn start_server(router: Router, port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();

    info!("🌐 Server starting on http://{}", addr);
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
