mod migrations;

use erno::{
    app::App,
    app_info::AppInfo,
    boot::{boot, BootConfig},
    jobs::job_registry::JobRegistry,
    jobs::scheduled_job::ScheduledJob,
};
use axum::{routing::get, Router};
use migrations::Migrator;

async fn health() -> &'static str {
    "OK"
}

fn router(_app: App) -> Router {
    Router::new().route("/health", get(health))
}

fn boot_config() -> BootConfig {
    BootConfig::new(
        AppInfo::new("{{name}}", env!("CARGO_PKG_VERSION"), ""),
        router,
        JobRegistry::new(),
        vec![],
    )
}

#[tokio::main]
async fn main() {
    boot::<Migrator, ()>(boot_config()).await;
}
