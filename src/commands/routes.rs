use axum::Router;
use std::collections::BTreeMap;

use crate::{
    app::App,
    environment::Environment,
    job_queue::JobQueue,
    mailer::Mailer,
    rate_limiting::{rate_limit_state::RateLimitConfig, RateLimitState},
    websocket::connections::Connections,
};

/// Handle the `routes` command - displays all registered application routes.
///
/// This command creates a minimal App instance and builds the router to display
/// the routes available in your application. It uses the router's debug output
/// to extract route information.
pub async fn handle_routes_command(app_router: fn(App) -> Router) {
    println!("📍 Application Routes\n");

    // Create a dummy app with minimal configuration to build the router
    let dummy_config = create_dummy_config();
    let dummy_app = create_dummy_app(dummy_config).await;

    // Build the full router
    let router = crate::router::router(dummy_app, app_router);

    // Extract and display routes
    extract_and_print_routes(router);
}

async fn create_dummy_app(config: crate::config::Config) -> App {
    // For route inspection, we don't actually need a real database connection
    // We use a special mock connection that won't be used
    let db = create_dummy_database_connection().await;

    App {
        config,
        environment: Environment::Development,
        db,
        mailer: Mailer::mock(),
        job_queue: JobQueue::mock(),
        rate_limit_state: RateLimitState::new(RateLimitConfig::default()),
        websocket_connections: Connections::new(),
    }
}

async fn create_dummy_database_connection() -> sea_orm::DatabaseConnection {
    use sea_orm::{ConnectOptions, Database};
    use std::time::Duration;

    // Create an in-memory SQLite connection for route inspection
    let mut opt = ConnectOptions::new("sqlite::memory:".to_string());
    opt.max_connections(1)
        .connect_timeout(Duration::from_secs(1))
        .acquire_timeout(Duration::from_secs(1));

    Database::connect(opt)
        .await
        .expect("Failed to create dummy database connection for route inspection")
}

fn create_dummy_config() -> crate::config::Config {
    use std::collections::HashMap;

    crate::config::Config {
        server: crate::config::ServerConfig { port: 3000 },
        database: crate::config::DatabaseConfig {
            url: "sqlite::memory:".to_string(),
            pool_size: 1,
        },
        base_url: "http://localhost:3000".to_string(),
        jwt: crate::config::JwtConfig {
            secret: "dummy_secret_for_route_inspection_only_1234567890".to_string(),
            expiration_days: 30,
        },
        password_reset: crate::config::PasswordResetConfig {
            token_expiration_hours: 24,
        },
        email: crate::config::EmailConfig::Mock,
        tracing: crate::config::TracingConfig {
            log_level: "error".to_string(),
        },
        jobs: crate::config::JobsConfig {
            cleanup: crate::config::CleanupConfig::default(),
            workers: crate::config::WorkersConfig {
                workers: HashMap::new(),
            },
        },
        rate_limiting: RateLimitConfig::default(),
    }
}

fn extract_and_print_routes(router: Router) {
    // Use debug output to extract routes
    let debug_output = format!("{:?}", router);

    // Uncomment for debugging:
    // eprintln!("Debug output:\n{}\n", debug_output);

    // Extract paths and their HTTP methods from the debug output
    let routes = extract_routes_with_methods(&debug_output);

    if routes.is_empty() {
        println!("No routes found. The router might be using nested or dynamic routing.");
        println!("\n💡 Tip: Check your app_router function implementation for route definitions.");
        return;
    }

    // Print header
    println!("{:<40} {:<40} DESCRIPTION", "METHOD(S)", "PATH");
    println!("{}", "─".repeat(100));

    // Group routes by path for better readability
    let mut routes_vec: Vec<_> = routes.into_iter().collect();
    routes_vec.sort_by(|a, b| a.0.cmp(&b.0));

    for (path, methods) in routes_vec {
        let description = match path.as_str() {
            "/liveness" => "Health check (liveness probe)",
            "/readiness" => "Health check (readiness probe)",
            "/ws" => "WebSocket endpoint",
            p if p.starts_with("/api/") => "Application endpoint",
            _ => "",
        };

        let methods_str = methods.join(", ");
        println!("{:<40} {:<40} {}", methods_str, path, description);
    }
}

fn extract_routes_with_methods(debug_output: &str) -> BTreeMap<String, Vec<String>> {
    let mut route_methods: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // First, extract the path mappings: RouteId -> path
    let mut route_id_to_path: BTreeMap<String, String> = BTreeMap::new();

    if let Some(paths_start) = debug_output.find("Node { paths: {") {
        let paths_section = &debug_output[paths_start..];
        if let Some(paths_end) = paths_section.find("} }") {
            let paths_content = &paths_section[15..paths_end];

            for part in paths_content.split("RouteId(") {
                if let Some(closing_paren) = part.find("):") {
                    let route_id = part[..closing_paren].trim().to_string();

                    if let Some(quote_start) = part.find('"') {
                        if let Some(quote_end) = part[quote_start + 1..].find('"') {
                            let path = &part[quote_start + 1..quote_start + 1 + quote_end];
                            if !path.contains("__private__") && !path.is_empty() && path != "/" {
                                route_id_to_path.insert(route_id, path.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Now extract the methods for each RouteId from the MethodRouter sections
    for (route_id, path) in route_id_to_path {
        // Look for the RouteId in the routes section with its MethodRouter
        let pattern = format!("RouteId({}): MethodRouter", route_id);
        if let Some(method_router_start) = debug_output.find(&pattern) {
            let method_section = &debug_output[method_router_start..];

            // Find the allow_header which contains the allowed methods
            if let Some(allow_header_start) = method_section.find("allow_header: Bytes(b\"") {
                const PREFIX: &str = "allow_header: Bytes(b\"";
                let allow_section = &method_section[allow_header_start + PREFIX.len()..];
                if let Some(allow_end) = allow_section.find('"') {
                    let methods_str = &allow_section[..allow_end];

                    // Parse methods; Axum includes HEAD when GET is present
                    let mut methods: Vec<String> = methods_str
                        .split(',')
                        .map(|m| m.trim().to_string())
                        .collect();

                    methods.dedup();

                    route_methods.insert(path, methods);
                }
            }
        }
    }

    route_methods
}
