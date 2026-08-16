use crate::{
    app::App,
    app_info::AppInfo,
    auth::jwt::generate_token,
    boot::{read_config, BootConfig},
    database::models::user,
    environment::Environment,
    jobs::job_registry::JobRegistry,
    mailer::Mailer,
    password::hash_password,
    rate_limiting::RateLimitState,
    router::router,
    websocket::connections::Connections,
};
use axum::Router;
use lettre::{transport::smtp::authentication::Credentials, AsyncSmtpTransport, Tokio1Executor};
use sea_orm::{ActiveModelTrait, ConnectOptions, ConnectionTrait, Set, Statement};
use sea_orm_migration::MigratorTrait;
use serde::de::DeserializeOwned;
use tokio::sync::OnceCell;
use tracing::debug;

static DB_SCHEMA_INITIALIZED: OnceCell<()> = OnceCell::const_new();
static TRACING_INITIALIZED: std::sync::Once = std::sync::Once::new();

fn init_tracing() {
    TRACING_INITIALIZED.call_once(|| {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

        tracing_subscriber::registry()
            .with(EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::layer().with_test_writer())
            .init();
    });
}

/// A fixture loader inserts committed baseline rows once, before any test
/// transaction. Prefer [`verified_user`] and other factories for per-example data.
pub type FixtureLoader =
    for<'a> fn(
        &'a sea_orm::DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;

/// Empty [`FixtureLoader`] — the usual choice.
pub fn no_fixtures(
    db: &sea_orm::DatabaseConnection,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
    Box::pin(async move {
        let _ = db;
    })
}

/// Minimal [`BootConfig`] for Erno's own tests (`ExtraConfig = ()`).
pub fn test_boot(app_router: fn(App) -> Router) -> BootConfig {
    BootConfig::new(
        AppInfo::new("test", "0", ""),
        app_router,
        JobRegistry::new(),
        vec![],
    )
}

/// Fallback when the test role cannot `DROP SCHEMA public` (DB owned by another
/// role). Drops tables, sequences, and types in `public` that this role owns.
async fn drop_owned_public_objects(db: &sea_orm::DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    db.execute_unprepared(
        r#"
        DO $$
        DECLARE r RECORD;
        BEGIN
          FOR r IN (SELECT tablename FROM pg_tables WHERE schemaname = 'public') LOOP
            BEGIN
              EXECUTE format('DROP TABLE IF EXISTS public.%I CASCADE', r.tablename);
            EXCEPTION WHEN insufficient_privilege THEN
              NULL;
            END;
          END LOOP;
          FOR r IN (
            SELECT sequence_name AS n FROM information_schema.sequences
            WHERE sequence_schema = 'public'
          ) LOOP
            BEGIN
              EXECUTE format('DROP SEQUENCE IF EXISTS public.%I CASCADE', r.n);
            EXCEPTION WHEN insufficient_privilege THEN
              NULL;
            END;
          END LOOP;
          FOR r IN (
            SELECT t.typname AS n
            FROM pg_type t
            JOIN pg_namespace n ON n.oid = t.typnamespace
            WHERE n.nspname = 'public' AND t.typtype IN ('e', 'c')
          ) LOOP
            BEGIN
              EXECUTE format('DROP TYPE IF EXISTS public.%I CASCADE', r.n);
            EXCEPTION WHEN insufficient_privilege THEN
              NULL;
            END;
          END LOOP;
          FOR r IN (
            SELECT p.proname AS n
            FROM pg_proc p
            JOIN pg_namespace n ON n.oid = p.pronamespace
            WHERE n.nspname = 'public'
          ) LOOP
            BEGIN
              EXECUTE format('DROP FUNCTION IF EXISTS public.%I CASCADE', r.n);
            EXCEPTION WHEN insufficient_privilege OR dependent_objects_still_exist THEN
              NULL;
            END;
          END LOOP;
        END $$;
        "#,
    )
    .await?;
    Ok(())
}

async fn initialize_database_schema<AppMigrator: MigratorTrait>(fixture_loader: FixtureLoader) {
    use crate::database::setup_database_connection;
    use tracing::{debug, error, info, trace};

    info!("Initializing test database schema (one-time setup)");

    let environment = Environment::Test;
    trace!("Reading test configuration");
    let app_config = read_config::<()>(&environment);

    debug!("Connecting to test database for schema setup");
    let db = setup_database_connection(&app_config.database).await;
    debug!("Database connection established");

    debug!("Resetting public schema");
    if let Err(e) = db
        .execute_unprepared(
            "DROP SCHEMA public CASCADE; CREATE SCHEMA public; GRANT ALL ON SCHEMA public TO public;",
        )
        .await
    {
        debug!("DROP SCHEMA failed ({e}); dropping objects the test role owns");
        if let Err(e) = drop_owned_public_objects(&db).await {
            error!("❌ Failed to reset test schema: {e}");
            panic!("Failed to reset test schema: {e}");
        }
    }

    debug!("Applying database migrations");
    match AppMigrator::up(&db, None).await {
        Ok(()) => {
            debug!("Database migrations applied successfully");
        }
        Err(e) => {
            error!("❌ Database migrations failed: {}", e);
            panic!("Database migrations failed: {e}");
        }
    }

    debug!("Loading test fixtures");
    fixture_loader(&db).await;
    debug!("Test fixtures loaded");

    info!("Test database schema initialization complete");
}

/// Boot a request-test server from the same [`BootConfig`] production uses.
///
/// Schema is created once; each call gets its own connection and a transaction
/// that rolls back when [`TestUtils`] is dropped.
///
/// # Panics
///
/// Panics if database setup or migrations fail.
pub async fn setup_test<AppMigrator, ExtraConfig>(
    boot: BootConfig<ExtraConfig>,
    fixture_loader: FixtureLoader,
) -> TestUtils
where
    AppMigrator: MigratorTrait,
    ExtraConfig: Clone + Send + Sync + DeserializeOwned + Default + 'static,
{
    init_tracing();

    debug!("Setting up test");

    DB_SCHEMA_INITIALIZED
        .get_or_init(|| async {
            debug!("Initializing database schema (first test only)");
            initialize_database_schema::<AppMigrator>(fixture_loader).await;
        })
        .await;

    let environment = Environment::Test;
    let app_config = read_config::<ExtraConfig>(&environment);
    // Token helpers only need the base config (`generate_token` reads `auth`).
    let base_config = read_config::<()>(&environment);

    debug!("Creating single-connection pool for test isolation");
    let db = {
        let mut options = ConnectOptions::new(app_config.database.url.clone());
        options.sqlx_logging(false);
        options.max_connections(1);
        options.min_connections(1);

        sea_orm::Database::connect(options)
            .await
            .expect("Failed to connect to the database")
    };

    debug!("Beginning transaction for test isolation");
    db.execute(Statement::from_string(db.get_database_backend(), "BEGIN"))
        .await
        .expect("Failed to begin transaction");

    let mailer = match &app_config.email {
        crate::config::EmailConfig::Mock => crate::mailer::Mailer::mock(),
        crate::config::EmailConfig::Smtp {
            host,
            port,
            username,
            password,
            ..
        } => {
            let mut mailer_builder = AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                .expect("Failed to create mailer transport")
                .port(*port);

            if let (Some(username), Some(password)) = (username, password) {
                mailer_builder = mailer_builder
                    .credentials(Credentials::new(username.clone(), password.clone()));
            }

            crate::mailer::Mailer::smtp(mailer_builder.build())
        }
    };

    let job_queue = crate::job_queue::JobQueue::mock();

    let rate_limit_state = crate::rate_limiting::RateLimitState::new(
        crate::rate_limiting::rate_limit_state::RateLimitConfig::default(),
    );

    let websocket_connections = Connections::new();

    let app = App {
        config: app_config.clone(),
        environment,
        db: db.clone(),
        mailer: mailer.clone(),
        job_queue: job_queue.clone(),
        sync_queue: crate::sync::queue::SyncQueue::mock(),
        sync_registry: std::sync::Arc::new(boot.sync_registry),
        rate_limit_state,
        websocket_connections: websocket_connections.clone(),
        storage: crate::storage::FileStorage::mock(),
        prometheus_handle: crate::metrics::setup_metrics(),
        metrics_collectors: std::sync::Arc::new(boot.metrics_collectors),
        job_failure_handler: boot.job_failure_handler,
        user_data_deleter: boot.user_data_deleter,
    };

    let test_router = router(app, boot.app_router);

    debug!("Creating test server");
    let server = axum_test::TestServer::new(test_router).expect("Failed to create test server");

    TestUtils {
        server,
        db,
        mailer,
        job_queue,
        websocket_connections,
        config: base_config,
        environment,
    }
}

/// Wrapper around `axum_test::TestServer` that also provides database access for tests.
///
/// Each test gets its own single-connection pool with a transaction that rolls
/// back on drop.
pub struct TestUtils {
    pub server: axum_test::TestServer,
    pub db: sea_orm::DatabaseConnection,
    pub mailer: Mailer,
    pub job_queue: crate::job_queue::JobQueue,
    /// The same `Connections` instance the app uses — lets tests observe
    /// live share fan-in/fan-out triggered by handlers.
    pub websocket_connections: Connections,
    pub config: crate::config::Config,
    pub environment: crate::environment::Environment,
}

/// Insert a verified email/password user.
pub async fn verified_user(
    db: &sea_orm::DatabaseConnection,
    email: &str,
    password: &str,
) -> user::Model {
    user::ActiveModel {
        email: Set(email.to_string()),
        password_hash: Set(Some(hash_password(password).expect("hash password"))),
        email_verified_at: Set(Some(chrono::Utc::now().naive_utc())),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert verified user")
}

/// Insert an unverified email/password user.
pub async fn unverified_user(
    db: &sea_orm::DatabaseConnection,
    email: &str,
    password: &str,
) -> user::Model {
    user::ActiveModel {
        email: Set(email.to_string()),
        password_hash: Set(Some(hash_password(password).expect("hash password"))),
        email_verified_at: Set(None),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert unverified user")
}

/// `Authorization: Bearer …` header value for `user`.
pub fn bearer(t: &TestUtils, user: &user::Model) -> String {
    format!(
        "Bearer {}",
        generate_token(&t.config, user.id, user.token_version).expect("sign test token")
    )
}

impl TestUtils {
    /// Get a reference to the underlying `axum_test::TestServer`.
    pub fn server(&self) -> &axum_test::TestServer {
        &self.server
    }

    /// Get sent emails from the mock mailer.
    pub fn sent_emails(&self) -> Vec<crate::mailer::MockEmailRecord> {
        self.mailer
            .records()
            .expect("Mock mailer should be used in tests")
    }

    /// Clear all sent emails from the mock mailer.
    pub fn clear_sent_emails(&self) {
        self.mailer.clear_messages();
    }

    /// Get all scheduled jobs from the mock job queue.
    pub fn enqueued_jobs(&self) -> Vec<crate::job_queue::EnqueuedJob> {
        self.job_queue
            .enqueued_jobs()
            .expect("Mock job queue should be used in tests")
    }

    /// Get scheduled jobs of a specific type from the mock job queue.
    pub fn enqueued_jobs_of_type(&self, job_type: &str) -> Vec<crate::job_queue::EnqueuedJob> {
        self.job_queue
            .enqueued_jobs_of_type(job_type)
            .expect("Mock job queue should be used in tests")
    }

    /// Clear all scheduled jobs from the mock job queue.
    pub fn clear_scheduled_jobs(&self) {
        self.job_queue.clear_scheduled_jobs();
    }

    /// Execute a job directly in tests.
    pub async fn execute_job<J: crate::jobs::Job>(
        &self,
        args: J::Arguments,
    ) -> Result<(), crate::jobs::JobError>
    where
        J::Arguments: serde::Serialize + serde::de::DeserializeOwned,
    {
        let app = App {
            config: self.config.clone(),
            environment: self.environment,
            db: self.db.clone(),
            mailer: self.mailer.clone(),
            job_queue: self.job_queue.clone(),
            sync_queue: crate::sync::queue::SyncQueue::mock(),
            sync_registry: std::sync::Arc::new(crate::sync::registry::SyncRegistry::new()),
            rate_limit_state: RateLimitState::new(self.config.rate_limiting.clone()),
            websocket_connections: Connections::new(),
            storage: crate::storage::FileStorage::mock(),
            prometheus_handle: crate::metrics::setup_metrics(),
            metrics_collectors: std::sync::Arc::new(
                crate::metrics::collector::CollectorRegistry::default(),
            ),
            job_failure_handler: None,
            user_data_deleter: None,
        };

        J::execute(&app, args).await
    }
}

impl Drop for TestUtils {
    fn drop(&mut self) {
        use tokio::runtime::Handle;

        let db = self.db.clone();
        if let Ok(handle) = Handle::try_current() {
            handle.spawn(async move {
                let _ = db
                    .execute(Statement::from_string(
                        db.get_database_backend(),
                        "ROLLBACK",
                    ))
                    .await;
            });
        }
    }
}
