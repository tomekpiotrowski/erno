use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sea_orm::DatabaseConnection;
use thiserror::Error;

use crate::{
    config::Config, database::DatabaseSetupStatus, environment::Environment, job_queue::JobQueue,
    jobs::Job, mailer::Mailer, rate_limiting::RateLimitState, websocket::connections::Connections,
};

#[derive(Clone)]
pub struct App<ExtraConfig = ()> {
    pub config: Config<ExtraConfig>,
    pub environment: Environment,
    pub db: DatabaseConnection,
    pub mailer: Mailer,
    pub job_queue: JobQueue,
    pub rate_limit_state: RateLimitState,
    pub websocket_connections: Connections,
}

impl<ExtraConfig> App<ExtraConfig> {
    pub async fn run_job<J>(&self, arguments: J::Arguments) -> Result<(), sea_orm::DbErr>
    where
        J: Job<ExtraConfig>,
        J::Arguments: serde::Serialize,
    {
        self.job_queue.add::<J, ExtraConfig>(&self.db, arguments).await
    }
}

#[derive(Debug, Error)]
pub enum ReadinessError {
    #[error("Database connection error")]
    DatabaseError(#[from] sea_orm::DbErr),
    #[error("Database setup error: {0}")]
    DatabaseSetupError(DatabaseSetupStatus),
}

impl IntoResponse for ReadinessError {
    fn into_response(self) -> Response {
        (StatusCode::SERVICE_UNAVAILABLE, self.to_string()).into_response()
    }
}
