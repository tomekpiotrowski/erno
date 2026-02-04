use axum::{extract::Path, routing::get, Json, Router};
use erno::{
    app::App,
    app_info::AppInfo,
    boot::{boot, BootConfig},
    jobs::job_registry::JobRegistry,
    jobs::scheduled_job::ScheduledJob,
};
use sea_orm_migration::{MigrationTrait, MigratorTrait};
use serde::{Deserialize, Serialize};

// Dummy migrator for the example
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![]
    }
}

#[derive(Serialize, Deserialize)]
struct User {
    id: u32,
    name: String,
    email: String,
}

#[derive(Serialize, Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

#[derive(Serialize)]
struct Post {
    id: u32,
    title: String,
    content: String,
}

// Handler functions
async fn list_users() -> Json<Vec<User>> {
    Json(vec![
        User {
            id: 1,
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
        },
        User {
            id: 2,
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
        },
    ])
}

async fn get_user(Path(id): Path<u32>) -> Json<User> {
    Json(User {
        id,
        name: format!("User {}", id),
        email: format!("user{}@example.com", id),
    })
}

async fn create_user(Json(_payload): Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 999,
        name: "New User".to_string(),
        email: "new@example.com".to_string(),
    })
}

async fn update_user(Path(id): Path<u32>, Json(_payload): Json<CreateUser>) -> Json<User> {
    Json(User {
        id,
        name: "Updated User".to_string(),
        email: "updated@example.com".to_string(),
    })
}

async fn delete_user(Path(id): Path<u32>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "success": true,
        "deleted_id": id
    }))
}

async fn list_posts() -> Json<Vec<Post>> {
    Json(vec![
        Post {
            id: 1,
            title: "First Post".to_string(),
            content: "This is the first post".to_string(),
        },
        Post {
            id: 2,
            title: "Second Post".to_string(),
            content: "This is the second post".to_string(),
        },
    ])
}

async fn get_post(Path(id): Path<u32>) -> Json<Post> {
    Json(Post {
        id,
        title: format!("Post {}", id),
        content: format!("Content of post {}", id),
    })
}

async fn health_check() -> &'static str {
    "OK"
}

// Application router
fn app_router(_app: App) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/users", get(list_users).post(create_user))
        .route(
            "/users/{id}",
            get(get_user).put(update_user).delete(delete_user),
        )
        .route("/posts", get(list_posts))
        .route("/posts/{id}", get(get_post))
}

fn job_registry() -> JobRegistry {
    JobRegistry::new()
}

fn job_schedule() -> Vec<ScheduledJob> {
    vec![]
}

fn boot_config() -> BootConfig {
    let app_info = AppInfo::new(
        "simple-api-example",
        env!("CARGO_PKG_VERSION"),
        "A simple API example demonstrating erno",
    );

    BootConfig::new(app_info, app_router, job_registry(), job_schedule())
}

#[tokio::main]
async fn main() {
    boot::<Migrator>(boot_config()).await;
}
