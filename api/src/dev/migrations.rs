use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use sea_orm::DatabaseConnection;
use sea_orm_migration::MigratorTrait;
use serde::Serialize;

use crate::app::App;

static CTL: OnceLock<Arc<dyn DevMigrator>> = OnceLock::new();

#[derive(Debug, Serialize)]
pub struct MigrationStatus {
    pub head: Option<String>,
    pub applied: Vec<String>,
    pub pending: Vec<String>,
}

#[async_trait]
trait DevMigrator: Send + Sync {
    async fn status(&self, db: &DatabaseConnection) -> Result<MigrationStatus, String>;
    async fn up_one(&self, db: &DatabaseConnection) -> Result<Option<String>, String>;
    async fn down_one(&self, db: &DatabaseConnection) -> Result<Option<String>, String>;
}

struct Ctl<M>(PhantomData<fn() -> M>);

#[async_trait]
impl<M: MigratorTrait + Send + 'static> DevMigrator for Ctl<M> {
    async fn status(&self, db: &DatabaseConnection) -> Result<MigrationStatus, String> {
        let applied = M::get_applied_migrations(db)
            .await
            .map_err(|e| e.to_string())?;
        let pending = M::get_pending_migrations(db)
            .await
            .map_err(|e| e.to_string())?;
        Ok(MigrationStatus {
            head: applied.last().map(|m| m.name().to_string()),
            applied: applied.iter().map(|m| m.name().to_string()).collect(),
            pending: pending.iter().map(|m| m.name().to_string()).collect(),
        })
    }

    async fn up_one(&self, db: &DatabaseConnection) -> Result<Option<String>, String> {
        let pending = M::get_pending_migrations(db)
            .await
            .map_err(|e| e.to_string())?;
        let Some(first) = pending.first() else {
            return Ok(None);
        };
        let name = first.name().to_string();
        M::up(db, Some(1)).await.map_err(|e| e.to_string())?;
        Ok(Some(name))
    }

    async fn down_one(&self, db: &DatabaseConnection) -> Result<Option<String>, String> {
        let applied = M::get_applied_migrations(db)
            .await
            .map_err(|e| e.to_string())?;
        let Some(last) = applied.last() else {
            return Ok(None);
        };
        let name = last.name().to_string();
        M::down(db, Some(1)).await.map_err(|e| e.to_string())?;
        Ok(Some(name))
    }
}

pub fn install<M: MigratorTrait + Send + 'static>() {
    let _ = CTL.set(Arc::new(Ctl::<M>(PhantomData)));
}

fn ctl() -> Option<&'static Arc<dyn DevMigrator>> {
    CTL.get()
}

pub async fn status<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    let Some(m) = ctl() else {
        return (StatusCode::NOT_IMPLEMENTED, "no migrator installed").into_response();
    };
    match m.status(&app.db).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Serialize)]
struct StepResult {
    applied: Option<String>,
    reverted: Option<String>,
}

pub async fn up<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    let Some(m) = ctl() else {
        return (StatusCode::NOT_IMPLEMENTED, "no migrator installed").into_response();
    };
    match m.up_one(&app.db).await {
        Ok(None) => (StatusCode::BAD_REQUEST, "nothing pending").into_response(),
        Ok(Some(name)) => Json(StepResult {
            applied: Some(name),
            reverted: None,
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn down<ExtraConfig: Clone + Send + Sync + 'static>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse {
    let Some(m) = ctl() else {
        return (StatusCode::NOT_IMPLEMENTED, "no migrator installed").into_response();
    };
    match m.down_one(&app.db).await {
        Ok(None) => (StatusCode::BAD_REQUEST, "nothing to revert").into_response(),
        Ok(Some(name)) => Json(StepResult {
            applied: None,
            reverted: Some(name),
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}
