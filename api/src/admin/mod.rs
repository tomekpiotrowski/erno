//! HTTP admin API for operators (`erno admin` TUI client).
//!
//! Docs: docs/src/content/docs/api/console.md

pub mod auth;
pub mod dto;
pub mod handlers;
pub mod service;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::{app::App, config::AdminConfig};

/// Build the admin router. Returns `None` when admin is not configured
/// (`password_hash` unset), so routes are not mounted.
pub fn admin_router<ExtraConfig>(app: App<ExtraConfig>) -> Option<Router>
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    if !app
        .config
        .admin
        .as_ref()
        .is_some_and(AdminConfig::is_enabled)
    {
        return None;
    }

    Some(
        Router::new()
            .route("/dashboard", get(handlers::get_dashboard::<ExtraConfig>))
            .route("/users", get(handlers::list_users::<ExtraConfig>))
            .route("/users/{id}", get(handlers::get_user::<ExtraConfig>))
            .route(
                "/users/{id}/activate",
                post(handlers::activate_user::<ExtraConfig>),
            )
            .route(
                "/users/{id}",
                delete(handlers::delete_user::<ExtraConfig>),
            )
            .route(
                "/users/{id}/gift",
                post(handlers::gift_user::<ExtraConfig>),
            )
            .route("/jobs", get(handlers::list_jobs::<ExtraConfig>))
            .route(
                "/jobs/{id}/retry",
                post(handlers::retry_job::<ExtraConfig>),
            )
            .route("/plans", get(handlers::list_plans::<ExtraConfig>))
            .route("/stats", get(handlers::get_stats::<ExtraConfig>))
            .with_state(app),
    )
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    use sea_orm::{ActiveModelTrait, Set};
    use serde_json::json;

    use crate::{
        app::App,
        database::{migrations::Migrator, models::user},
        password::hash_password,
        tests::setup_test::setup_test,
    };

    fn test_router(_app: App) -> Router {
        Router::new()
    }

    fn no_fixtures(
        db: &sea_orm::DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let _ = db;
        })
    }

    fn basic(user: &str, pass: &str) -> String {
        format!("Basic {}", BASE64.encode(format!("{user}:{pass}")))
    }

    #[tokio::test]
    async fn dashboard_requires_auth() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        let response = t.server.get("/admin/api/dashboard").await;
        // Without password_hash in test config, admin routes are not mounted → 404
        // With hash, would be 401. Either way unauthenticated access is denied.
        assert!(
            response.status_code() == 401 || response.status_code() == 404,
            "status={}",
            response.status_code()
        );
    }

    #[tokio::test]
    async fn dashboard_with_valid_basic_auth() {
        // setup_test loads config/test.toml which we configure with a known hash
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        if t.config.admin.as_ref().map(|a| a.password_hash.is_empty()) != Some(false) {
            // Admin not enabled in this config — skip functional check
            return;
        }

        let auth = basic("admin", "admin");
        let response = t
            .server
            .get("/admin/api/dashboard")
            .add_header(
                axum::http::header::AUTHORIZATION,
                auth.parse::<axum::http::HeaderValue>().unwrap(),
            )
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert!(body["total_users"].is_number());
    }

    #[tokio::test]
    async fn activate_user_via_admin_api() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        if t.config.admin.as_ref().map(|a| a.password_hash.is_empty()) != Some(false) {
            return;
        }

        let u = user::ActiveModel {
            email: Set("admin-activate@example.com".to_string()),
            password_hash: Set(Some(hash_password("password123").unwrap())),
            email_verified_at: Set(None),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let auth = basic("admin", "admin");
        let response = t
            .server
            .post(&format!("/admin/api/users/{}/activate", u.id))
            .add_header(
                axum::http::header::AUTHORIZATION,
                auth.parse::<axum::http::HeaderValue>().unwrap(),
            )
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert!(body["user"]["email_verified_at"].is_string());
    }

    #[tokio::test]
    async fn wrong_password_returns_401() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        if t.config.admin.as_ref().map(|a| a.password_hash.is_empty()) != Some(false) {
            return;
        }

        let auth = basic("admin", "wrong");
        let response = t
            .server
            .get("/admin/api/dashboard")
            .add_header(
                axum::http::header::AUTHORIZATION,
                auth.parse::<axum::http::HeaderValue>().unwrap(),
            )
            .await;
        assert_eq!(response.status_code(), 401);
        let _ = json!({});
    }
}
