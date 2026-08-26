//! Collector route table.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md

use std::sync::Arc;

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post},
    Router,
};

use crate::{app::App, error_reporting::config::CollectorConfig};

use super::{handlers, ingest::CollectorSink, operator, state::CollectorState};

/// How often retention sweeps. Hourly matches the framework's job cleanup.
const RETENTION_INTERVAL_SECONDS: u64 = 3_600;

/// Build the collector's routes, or `None` when the collector is switched off.
///
/// Returning `None` rather than an empty router follows the same idiom as
/// [`crate::admin::admin_router`]: a disabled feature mounts nothing at all, so
/// its endpoints 404 rather than existing in a half-working state.
///
/// The monitoring binary merges this into its `app_router`, which the framework
/// nests under `/api` — so ingest lands at `POST /api/errors`.
#[must_use]
pub fn collector_router<ExtraConfig>(
    app: App<ExtraConfig>,
    config: CollectorConfig,
) -> Option<Router>
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    if !config.enabled {
        return None;
    }

    let max_body_bytes = config.max_body_bytes;

    // Alerting needs the mailer and the envelope sender, which only the host
    // application knows.
    let alert_sender = match &app.config.email {
        crate::config::EmailConfig::Smtp { sender, .. } => sender.to_string(),
        crate::config::EmailConfig::Mock => "noreply@example.com".to_string(),
    };
    // Where an operator should land: the console, not the API.
    let console_url = app
        .config
        .app_url
        .clone()
        .unwrap_or_else(|| app.config.api_url.clone());

    let alerts = config.alerts.is_active().then(|| {
        (
            app.mailer.clone(),
            super::alerts::AlertContext {
                config: config.alerts.clone(),
                sender: alert_sender.clone(),
                console_url: console_url.clone(),
                templates_dir: app.config.email_templates_dir.clone(),
            },
        )
    });

    // Retention runs on a deployment-wide singleton lock, so replicas do not
    // duplicate the sweep. Skipped under `sync_writes`, which is the test mode.
    if !config.sync_writes {
        // Probing from the test harness would make outbound HTTP requests from
        // every test run.
        super::uptime::spawn_runner(app.db.clone());
        super::status::spawn_publisher(app.db.clone(), config.status.clone());
        super::alerting::spawn_alerting(
            app.db.clone(),
            app.mailer.clone(),
            super::alerting::notifier::NotifyContext {
                sender: alert_sender.clone(),
                default_recipient: (!config.alerts.recipient.trim().is_empty())
                    .then(|| config.alerts.recipient.clone()),
                console_url: console_url.clone(),
            },
            config.health.clone(),
            config.prometheus.url().map(ToString::to_string),
        );
        super::retention::spawn(
            app.db.clone(),
            config.clone(),
            std::time::Duration::from_secs(RETENTION_INTERVAL_SECONDS),
        );
    }

    let config = Arc::new(config);
    let sink = CollectorSink::start(app.db.clone(), Arc::clone(&config), alerts);

    let state = CollectorState { app, config, sink };
    let public_state = state.clone();

    // Ingest: token-authenticated, high volume, generous body limit.
    let ingest = Router::new()
        .route("/errors", post(handlers::ingest::<ExtraConfig>))
        .route("/otlp/auth", get(handlers::otlp_auth::<ExtraConfig>))
        // Applied to the ingest route only, not globally: other routes on this
        // deployment have no reason to accept 64 KiB bodies.
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .with_state(state.clone());

    // Operator console: HTTP Basic, applied as a layer so no route can be added
    // later without it.
    let operator_routes = Router::new()
        .route("/issues", get(operator::list_issues::<ExtraConfig>))
        .route("/issues/counts", get(operator::issue_counts::<ExtraConfig>))
        .route(
            "/issues/{id}",
            get(operator::get_issue::<ExtraConfig>).delete(operator::delete_issue::<ExtraConfig>),
        )
        .route(
            "/issues/{id}/events",
            get(operator::list_events::<ExtraConfig>),
        )
        .route(
            "/issues/{id}/series",
            get(operator::issue_series::<ExtraConfig>),
        )
        .route(
            "/issues/{id}/resolve",
            post(operator::resolve::<ExtraConfig>),
        )
        .route("/issues/{id}/ignore", post(operator::ignore::<ExtraConfig>))
        .route(
            "/issues/{id}/unresolve",
            post(operator::unresolve::<ExtraConfig>),
        )
        .route("/series", get(operator::global_series::<ExtraConfig>))
        .route("/releases", get(operator::list_releases::<ExtraConfig>))
        .route("/health", get(operator::get_health::<ExtraConfig>))
        .route(
            "/uptime",
            get(operator::list_checks::<ExtraConfig>).post(operator::create_check::<ExtraConfig>),
        )
        .route(
            "/uptime/{id}",
            delete(operator::delete_check::<ExtraConfig>),
        )
        .route(
            "/uptime/{id}/enable",
            post(operator::enable_check::<ExtraConfig>),
        )
        .route(
            "/uptime/{id}/disable",
            post(operator::disable_check::<ExtraConfig>),
        )
        .route(
            "/status/components",
            get(operator::list_components::<ExtraConfig>)
                .post(operator::create_component::<ExtraConfig>),
        )
        .route(
            "/status/components/{id}",
            delete(operator::delete_component::<ExtraConfig>),
        )
        .route(
            "/status/components/{id}/state",
            post(operator::set_component_state::<ExtraConfig>),
        )
        .route(
            "/status/incidents",
            post(operator::open_incident::<ExtraConfig>),
        )
        .route(
            "/status/incidents/{id}/updates",
            post(operator::add_incident_update::<ExtraConfig>),
        )
        .route(
            "/alerts",
            get(operator::list_rules::<ExtraConfig>).post(operator::create_rule::<ExtraConfig>),
        )
        .route("/alerts/{id}", delete(operator::delete_rule::<ExtraConfig>))
        .route(
            "/alerts/{id}/enable",
            post(operator::enable_rule::<ExtraConfig>),
        )
        .route(
            "/alerts/{id}/disable",
            post(operator::disable_rule::<ExtraConfig>),
        )
        .route(
            "/alerts/{id}/silence",
            post(operator::silence_rule::<ExtraConfig>),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            operator::require_operator::<ExtraConfig>,
        ))
        .with_state(state.clone());

    // Machine-to-machine: authenticated by the trusted ingest token, not by
    // operator credentials, because an application calls it during account
    // deletion.
    let machine_routes = Router::new()
        .route(
            "/users/{id}/events",
            delete(operator::anonymize_user::<ExtraConfig>),
        )
        .route("/releases", post(operator::record_release::<ExtraConfig>))
        .route("/health", post(operator::record_health::<ExtraConfig>))
        .with_state(state);

    // Deliberately unauthenticated, and deliberately outside the operator
    // layer: this previews a *public* document. Relying on it in production
    // defeats the point, since a status page served by the collector goes down
    // with the collector.
    let public_routes = Router::new()
        .route(
            "/status.json",
            get(operator::status_snapshot::<ExtraConfig>),
        )
        .with_state(public_state);

    Some(ingest.merge(Router::new().nest(
        "/collector",
        operator_routes.merge(machine_routes).merge(public_routes),
    )))
}
