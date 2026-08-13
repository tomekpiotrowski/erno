//! HTTP admin API for the operator web app.
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
            .route("/users/{id}", delete(handlers::delete_user::<ExtraConfig>))
            .route("/users/{id}/gift", post(handlers::gift_user::<ExtraConfig>))
            .route("/jobs", get(handlers::list_jobs::<ExtraConfig>))
            .route("/jobs/{id}", get(handlers::get_job::<ExtraConfig>))
            .route("/jobs/{id}/retry", post(handlers::retry_job::<ExtraConfig>))
            .route("/emails", get(handlers::list_emails::<ExtraConfig>))
            .route("/emails/{id}", get(handlers::get_email::<ExtraConfig>))
            .route("/tables", get(handlers::list_tables::<ExtraConfig>))
            .route("/events", get(handlers::list_events::<ExtraConfig>))
            .route("/plans", get(handlers::list_plans::<ExtraConfig>))
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
    async fn delete_user_keeps_admin_event() {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        if t.config.admin.as_ref().map(|a| a.password_hash.is_empty()) != Some(false) {
            return;
        }

        let u = user::ActiveModel {
            email: Set("admin-delete@example.com".to_string()),
            password_hash: Set(Some(hash_password("password123").unwrap())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();
        let user_id = u.id;

        let auth = basic("admin", "admin");
        let response = t
            .server
            .delete(&format!("/admin/api/users/{user_id}"))
            .add_header(
                axum::http::header::AUTHORIZATION,
                auth.parse::<axum::http::HeaderValue>().unwrap(),
            )
            .await;
        assert_eq!(response.status_code(), 204);

        assert!(user::Entity::find_by_id(user_id)
            .one(&t.db)
            .await
            .unwrap()
            .is_none());

        let events = crate::database::models::admin_event::Entity::find()
            .filter(crate::database::models::admin_event::Column::Name.eq("user.deleted"))
            .filter(crate::database::models::admin_event::Column::UserId.eq(user_id))
            .all(&t.db)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
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

    fn auth_header() -> axum::http::HeaderValue {
        basic("admin", "admin")
            .parse::<axum::http::HeaderValue>()
            .unwrap()
    }

    #[tokio::test]
    async fn list_users_paginates_and_includes_last_active() {
        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        if t.config.admin.as_ref().map(|a| a.password_hash.is_empty()) != Some(false) {
            return;
        }

        let now = chrono::Utc::now().naive_utc();
        user::ActiveModel {
            email: Set("page-a@example.com".to_string()),
            password_hash: Set(Some(hash_password("password123").unwrap())),
            last_active_at: Set(Some(now)),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();
        user::ActiveModel {
            email: Set("page-b@example.com".to_string()),
            password_hash: Set(Some(hash_password("password123").unwrap())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let response = t
            .server
            .get("/admin/api/users?per_page=1&page=1&q=page-")
            .add_header(axum::http::header::AUTHORIZATION, auth_header())
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert_eq!(body["users"].as_array().unwrap().len(), 1);
        assert_eq!(body["per_page"], 1);
        assert!(body["total"].as_u64().unwrap() >= 2);
        assert!(body["users"][0].get("last_active_at").is_some());
    }

    #[tokio::test]
    async fn emails_and_events_and_tables_and_job_detail() {
        use crate::database::models::{email_message, job, job_status::JobStatus};

        let t = setup_test::<Migrator>(test_router, no_fixtures).await;
        if t.config.admin.as_ref().map(|a| a.password_hash.is_empty()) != Some(false) {
            return;
        }

        let response = t
            .server
            .post("/api/auth/register")
            .json(&json!({ "email": "admin-api@example.com", "password": "password123" }))
            .await;
        assert_eq!(response.status_code(), 201);

        let events = t
            .server
            .get("/admin/api/events?name=user.registered")
            .add_header(axum::http::header::AUTHORIZATION, auth_header())
            .await;
        assert_eq!(events.status_code(), 200);
        let events_body: serde_json::Value = events.json();
        assert!(!events_body["events"].as_array().unwrap().is_empty());

        let now = chrono::Utc::now().naive_utc();
        let mail = email_message::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            to: Set("admin-api@example.com".to_string()),
            from: Set("noreply@example.com".to_string()),
            subject: Set("Hello".to_string()),
            template: Set(Some("verification".to_string())),
            user_id: Set(None),
            job_id: Set(None),
            status: Set("sent".to_string()),
            error: Set(None),
            sent_at: Set(Some(now)),
            created_at: Set(now),
        }
        .insert(&t.db)
        .await
        .unwrap();

        let emails = t
            .server
            .get("/admin/api/emails?to=admin-api")
            .add_header(axum::http::header::AUTHORIZATION, auth_header())
            .await;
        assert_eq!(emails.status_code(), 200);
        let emails_body: serde_json::Value = emails.json();
        assert!(emails_body["total"].as_u64().unwrap() >= 1);

        let email_one = t
            .server
            .get(&format!("/admin/api/emails/{}", mail.id))
            .add_header(axum::http::header::AUTHORIZATION, auth_header())
            .await;
        assert_eq!(email_one.status_code(), 200);

        let inserted_job = job::ActiveModel {
            r#type: Set("send_verification_email".to_string()),
            arguments: Set(json!({ "email": "admin-api@example.com" })),
            status: Set(JobStatus::Completed),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        let job_one = t
            .server
            .get(&format!("/admin/api/jobs/{}", inserted_job.id))
            .add_header(axum::http::header::AUTHORIZATION, auth_header())
            .await;
        assert_eq!(job_one.status_code(), 200);
        let job_body: serde_json::Value = job_one.json();
        assert!(job_body["arguments"].is_object());
        assert!(job_body["executions"].is_array());

        let tables = t
            .server
            .get("/admin/api/tables")
            .add_header(axum::http::header::AUTHORIZATION, auth_header())
            .await;
        assert_eq!(tables.status_code(), 200);
        let tables_body: serde_json::Value = tables.json();
        assert!(tables_body["tables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| { row["table"] == "users" && row["approx"] == true }));
    }
}
