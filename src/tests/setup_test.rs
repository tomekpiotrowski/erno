use crate::{
    app::App,
    boot::read_config,
    environment::Environment,
    mailer::Mailer,
    rate_limiting::{rate_limit_state::RateLimitConfig, RateLimitState},
    router::router,
    websocket::connections::Connections,
};
use axum::Router;
use lettre::{transport::smtp::authentication::Credentials, AsyncSmtpTransport, Tokio1Executor};
use sea_orm::{ConnectOptions, ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;
use tokio::sync::OnceCell;
use tracing::debug;

static DB_SCHEMA_INITIALIZED: OnceCell<()> = OnceCell::const_new();
static TRACING_INITIALIZED: std::sync::Once = std::sync::Once::new();

/// Initialize tracing for tests
fn init_tracing() {
    TRACING_INITIALIZED.call_once(|| {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

        tracing_subscriber::registry()
            .with(EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::layer().with_test_writer())
            .init();
    });
}

/// Type alias for a fixture loader function.
///
/// A fixture loader is an async function that inserts all test fixtures
/// into the database connection (not transaction).
pub type FixtureLoader =
    for<'a> fn(
        &'a sea_orm::DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;

/// Drop and recreate the database schema.
///
/// This provides a completely clean slate by removing all tables, types,
/// functions, and other database objects from the public schema.
async fn reset_database_schema(db: &DatabaseConnection) {
    use tracing::{debug, trace};

    debug!("Resetting database schema");

    // Drop everything in the public schema and recreate it
    trace!("Dropping public schema");
    let drop_schema = "DROP SCHEMA public CASCADE";
    db.execute(Statement::from_string(DbBackend::Postgres, drop_schema))
        .await
        .expect("Failed to drop public schema");

    trace!("Creating public schema");
    let create_schema = "CREATE SCHEMA public";
    db.execute(Statement::from_string(DbBackend::Postgres, create_schema))
        .await
        .expect("Failed to create public schema");

    trace!("Granting permissions on public schema");
    // Grant usage on public schema
    let grant_usage = "GRANT ALL ON SCHEMA public TO PUBLIC";
    db.execute(Statement::from_string(DbBackend::Postgres, grant_usage))
        .await
        .expect("Failed to grant permissions on public schema");

    debug!("Database schema reset complete");
}

/// Initialize the database schema once for all tests.
///
/// Drops and recreates the schema, runs migrations, and loads fixtures once.
/// This ensures a completely clean database state before any tests run.
/// Each test will get its own connection to this initialized database.
async fn initialize_database_schema<AppMigrator: MigratorTrait>(fixture_loader: FixtureLoader) {
    use crate::{boot::read_config, database::setup_database_connection, environment::Environment};
    use tracing::{debug, error, info, trace};

    info!("Initializing test database schema (one-time setup)");

    let environment = Environment::Test;
    trace!("Reading test configuration");
    let app_config = read_config(&environment);

    // Connect to database for schema initialization
    debug!("Connecting to test database for schema setup");
    let db = setup_database_connection(&app_config.database).await;
    debug!("Database connection established");

    // Drop and recreate the entire schema for a clean slate
    reset_database_schema(&db).await;

    // Run migrations
    debug!("Running database migrations");
    match AppMigrator::up(&db, None).await {
        Ok(()) => {
            debug!("Database migrations completed successfully");
        }
        Err(e) => {
            error!("❌ Database migrations failed: {}", e);
            panic!("Database migrations failed: {e}");
        }
    }

    // Load all fixtures once
    debug!("Loading test fixtures");
    fixture_loader(&db).await;
    debug!("Test fixtures loaded");

    info!("Test database schema initialization complete");
}

/// Creates a test server for integration testing.
///
/// Sets up the application with the test environment and returns a `TestUtils`
/// that provides both an `axum_test::TestServer` for making requests and access to the
/// database transaction for test assertions.
///
/// This function:
/// 1. Drops and recreates the database schema once (during first initialization)
/// 2. Runs migrations once
/// 3. Loads all fixtures once (during first initialization)
/// 4. Creates a new database connection for this specific test
/// 5. Begins a transaction for test isolation
///
/// Each test gets its own database connection, allowing parallel test execution.
///
/// # Panics
///
/// Panics if database setup or migrations fail.
pub async fn setup_test<AppMigrator: MigratorTrait>(
    app_router: fn(App) -> Router,
    fixture_loader: FixtureLoader,
) -> TestUtils {
    // Initialize tracing for test output
    init_tracing();

    debug!("Setting up test");

    // Initialize database schema once (drops schema, runs migrations, loads fixtures)
    // This uses tokio::sync::OnceCell to ensure it only runs once across all tests
    DB_SCHEMA_INITIALIZED
        .get_or_init(|| async {
            debug!("Initializing database schema (first test only)");
            initialize_database_schema::<AppMigrator>(fixture_loader).await;
        })
        .await;

    // Create a NEW connection for this specific test with a SINGLE connection pool
    // This ensures all queries go through the same connection, enabling transaction isolation
    let environment = Environment::Test;
    let app_config = read_config(&environment);

    debug!("Creating single-connection pool for test isolation");
    let db = {
        let mut options = ConnectOptions::new(app_config.database.url.clone());
        options.sqlx_logging(false);
        // Use exactly 1 connection so all queries go through the same connection
        options.max_connections(1);
        options.min_connections(1);

        sea_orm::Database::connect(options)
            .await
            .expect("Failed to connect to the database")
    };

    // Begin a transaction manually - since we have only 1 connection,
    // all subsequent queries will be within this transaction
    debug!("Beginning transaction for test isolation");
    db.execute(Statement::from_string(DbBackend::Postgres, "BEGIN"))
        .await
        .expect("Failed to begin transaction");

    // Create mailer based on config (mock or real SMTP)
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

    // Use mock job queue for tests
    let job_queue = crate::job_queue::JobQueue::mock();

    // Initialize rate limiting with default config for tests
    let rate_limit_state = RateLimitState::new(RateLimitConfig::default());

    // Initialize WebSocket connections for tests
    let websocket_connections = Connections::new();

    let app = App {
        config: app_config.clone(),
        environment,
        db: db.clone(),
        mailer: mailer.clone(),
        job_queue: job_queue.clone(),
        rate_limit_state,
        websocket_connections,
    };

    let test_router = router(app, app_router);

    debug!("Creating test server");
    let server = axum_test::TestServer::new(test_router).expect("Failed to create test server");

    TestUtils {
        server,
        db,
        mailer,
        job_queue,
        config: app_config,
        environment,
    }
}

/// Wrapper around `axum_test::TestServer` that also provides database access for tests.
///
/// # Transaction Isolation
///
/// Each test gets its own single-connection database pool with a manually started
/// transaction. This ensures:
/// - All queries (both from tests and the app) go through the same connection
/// - All changes are automatically rolled back when the test completes
/// - Tests are fully isolated from each other
///
/// Simply use `&test.db` for all database operations - they're all within the transaction.
pub struct TestUtils {
    pub server: axum_test::TestServer,
    pub db: sea_orm::DatabaseConnection,
    pub mailer: Mailer,
    pub job_queue: crate::job_queue::JobQueue,
    pub config: crate::config::Config,
    pub environment: crate::environment::Environment,
}

impl TestUtils {
    /// Get a reference to the underlying `axum_test::TestServer`.
    pub fn server(&self) -> &axum_test::TestServer {
        &self.server
    }

    /// Get sent emails from the mock mailer.
    ///
    /// Returns an empty vector if no emails have been sent.
    /// Panics if called with a real SMTP mailer (should only happen in tests).
    pub fn sent_emails(&self) -> Vec<lettre::Message> {
        self.mailer
            .messages()
            .expect("Mock mailer should be used in tests")
    }

    /// Clear all sent emails from the mock mailer.
    pub fn clear_sent_emails(&self) {
        self.mailer.clear_messages();
    }

    /// Get all scheduled jobs from the mock job queue.
    ///
    /// Returns None if using the real database queue.
    /// Panics if called outside of tests with mock queue.
    pub fn enqueued_jobs(&self) -> Vec<crate::job_queue::EnqueuedJob> {
        self.job_queue
            .enqueued_jobs()
            .expect("Mock job queue should be used in tests")
    }

    /// Get scheduled jobs of a specific type from the mock job queue.
    ///
    /// Returns None if using the real database queue.
    /// Panics if called outside of tests with mock queue.
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
    ///
    /// This creates an App instance from the test context and executes the job.
    /// The job will use the test's database connection, mock mailer, and mock job queue.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use crate::jobs::verification_email_job::{VerificationEmailJob, VerificationEmailJobArguments};
    ///
    /// let test = test().await;
    /// let args = VerificationEmailJobArguments {
    ///     user_id: user.id.to_string(),
    /// };
    ///
    /// // Execute the job and check result
    /// let result = test.execute_job::<VerificationEmailJob>(args).await;
    /// assert!(result.is_ok());
    ///
    /// // Verify email was sent
    /// assert_eq!(test.sent_emails().len(), 1);
    /// ```
    pub async fn execute_job<J: crate::jobs::Job>(
        &self,
        args: J::Arguments,
    ) -> Result<(), crate::jobs::JobError>
    where
        J::Arguments: serde::Serialize + serde::de::DeserializeOwned,
    {
        // Create an App instance using the test database connection
        // Jobs use the transaction implicitly through test queries
        let app = App {
            config: self.config.clone(),
            environment: self.environment,
            db: self.db.clone(),
            mailer: self.mailer.clone(),
            job_queue: self.job_queue.clone(),
            rate_limit_state: RateLimitState::new(self.config.rate_limiting.clone()),
            websocket_connections: Connections::new(),
        };

        J::execute(&app, args).await
    }
}

impl Drop for TestUtils {
    fn drop(&mut self) {
        // Rollback the transaction when the test completes
        // This ensures test isolation by undoing all database changes
        //
        // Note: We use spawn_blocking because Drop is sync but we need async.
        // The ROLLBACK will execute even if the test panicked.
        use tokio::runtime::Handle;

        let db = self.db.clone();
        if let Ok(handle) = Handle::try_current() {
            handle.spawn(async move {
                let _ = db
                    .execute(Statement::from_string(DbBackend::Postgres, "ROLLBACK"))
                    .await;
            });
        }
    }
}
