use std::fmt::{self, Display, Formatter};

use sea_orm::{ConnectOptions, DbErr};
use sea_orm_migration::MigratorTrait;
use tokio::sync::oneshot;
use tracing::debug;

use crate::config::DatabaseConfig;

pub mod migrations;
pub(crate) mod models;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum DatabaseSetupStatus {
    MigrationsInProgress,
    MigrationsFailed(String),
    Completed,
}

impl Display for DatabaseSetupStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MigrationsInProgress => write!(f, "Migrations in progress"),
            Self::MigrationsFailed(e) => write!(f, "Migrations failed: {e}"),
            Self::Completed => write!(f, "Database setup completed"),
        }
    }
}

pub async fn setup_database<AppMigrator: MigratorTrait>(
    db_config: &DatabaseConfig,
) -> (
    sea_orm::DatabaseConnection,
    oneshot::Receiver<Result<(), DbErr>>,
) {
    let connection = setup_database_connection(db_config).await;
    let migrations_connection = connection.clone();

    let (sender, receiver) = oneshot::channel();

    tokio::spawn(async move {
        let migration_result = AppMigrator::up(&migrations_connection, None).await;
        let _ = sender.send(migration_result);
    });

    (connection, receiver)
}

pub async fn setup_database_connection(db_config: &DatabaseConfig) -> sea_orm::DatabaseConnection {
    let mut options = ConnectOptions::new(db_config.url.clone());

    options.sqlx_logging(false); // Disable SQL query logging to reduce noise
    options.max_connections(db_config.pool_size);

    debug!("Connecting to database at: {}", &db_config.url);

    sea_orm::Database::connect(options)
        .await
        .expect("Failed to connect to the database")
}
