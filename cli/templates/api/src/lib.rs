mod migrations;

use axum::{routing::get, Router};
use erno::{app::App, app_info::AppInfo, boot::BootConfig, jobs::job_registry::JobRegistry};

pub use migrations::Migrator;

async fn health() -> &'static str {
    "OK"
}

pub fn router(_app: App) -> Router {
    Router::new().route("/health", get(health))
}

pub fn boot_config() -> BootConfig {
    BootConfig::new(
        AppInfo::new("{{name}}", env!("CARGO_PKG_VERSION"), ""),
        router,
        JobRegistry::new(),
        vec![],
    )
}
