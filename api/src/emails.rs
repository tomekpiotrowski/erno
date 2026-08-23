//! Docs: docs/src/content/docs/api/email.md
use chrono::Utc;
use lettre::{
    message::{header::ContentType, MultiPart},
    Message,
};
use sea_orm::{ActiveModelTrait, Set};
use thiserror::Error;
use uuid::Uuid;

use crate::{app::App, database::models::email_message, jobs::JobError, mailer::MockEmailRecord};

/// Optional outbox metadata. Bodies are never stored.
#[derive(Debug, Clone, Default)]
pub struct EmailMeta {
    pub template: Option<String>,
    pub user_id: Option<Uuid>,
    pub job_id: Option<Uuid>,
}

#[derive(Error, Debug)]
pub enum EmailError {
    #[error("Invalid recipient address: {0}")]
    InvalidRecipient(#[from] lettre::address::AddressError),
    #[error("Failed to build email: {0}")]
    BuilderError(#[from] lettre::error::Error),
    #[error("Failed to send email: {0}")]
    TransportError(#[from] lettre::transport::smtp::Error),
    #[error("Template error: {0}")]
    TemplateError(String),
    #[error("Mailer error: {0}")]
    MailerError(String),
}

impl From<EmailError> for JobError {
    fn from(error: EmailError) -> Self {
        match error {
            EmailError::InvalidRecipient(e) => JobError::FailPermanently(e.to_string()),
            EmailError::BuilderError(e) => JobError::TryAgainLater(e.to_string()),
            EmailError::TransportError(e) => JobError::TryAgainLater(e.to_string()),
            EmailError::TemplateError(e) => JobError::FailPermanently(e),
            EmailError::MailerError(e) => JobError::TryAgainLater(e),
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for EmailError {
    fn from(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        EmailError::MailerError(error.to_string())
    }
}

pub async fn send_html_email<ExtraConfig>(
    app: &App<ExtraConfig>,
    recipient: &str,
    subject: &str,
    body: String,
) -> Result<(), EmailError> {
    send_html_email_with_meta(app, recipient, subject, body, EmailMeta::default()).await
}

pub async fn send_html_email_with_meta<ExtraConfig>(
    app: &App<ExtraConfig>,
    recipient: &str,
    subject: &str,
    body: String,
    meta: EmailMeta,
) -> Result<(), EmailError> {
    let sender = match &app.config.email {
        crate::config::EmailConfig::Smtp { sender, .. } => sender.clone(),
        crate::config::EmailConfig::Mock => {
            "noreply@example.com".parse().expect("Invalid mock sender")
        }
    };

    let from = sender.to_string();

    app.mailer.store_record(MockEmailRecord {
        id: uuid::Uuid::new_v4(),
        to: recipient.to_string(),
        from: from.clone(),
        subject: subject.to_string(),
        body_html: Some(body.clone()),
        body_text: None,
        created_at: chrono::Utc::now(),
    });

    let email = Message::builder()
        .from(sender)
        .to(recipient.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(body)?;

    let timer = crate::metrics::OperationTimer::start(
        "erno_email_send_duration_seconds",
        "erno_email_send_total",
        "template",
        meta.template.clone().unwrap_or_else(|| "none".to_string()),
    );
    let send_result = app
        .mailer
        .send(email)
        .await
        .map_err(|e| EmailError::MailerError(e.to_string()));
    timer.finish(&send_result);

    persist_email_message(app, recipient, &from, subject, &meta, &send_result).await;

    send_result
}

/// Sends a multipart email with both plain text and HTML versions.
///
/// This is the preferred method for sending emails as it provides better
/// accessibility and compatibility. Email clients will automatically choose
/// the best format for the user.
pub async fn send_multipart_email<ExtraConfig>(
    app: &App<ExtraConfig>,
    recipient: &str,
    subject: &str,
    text_body: String,
    html_body: String,
) -> Result<(), EmailError> {
    send_multipart_email_with_meta(
        app,
        recipient,
        subject,
        text_body,
        html_body,
        EmailMeta::default(),
    )
    .await
}

pub async fn send_multipart_email_with_meta<ExtraConfig>(
    app: &App<ExtraConfig>,
    recipient: &str,
    subject: &str,
    text_body: String,
    html_body: String,
    meta: EmailMeta,
) -> Result<(), EmailError> {
    let sender = match &app.config.email {
        crate::config::EmailConfig::Smtp { sender, .. } => sender.clone(),
        crate::config::EmailConfig::Mock => {
            "noreply@example.com".parse().expect("Invalid mock sender")
        }
    };

    let from = sender.to_string();

    app.mailer.store_record(MockEmailRecord {
        id: uuid::Uuid::new_v4(),
        to: recipient.to_string(),
        from: from.clone(),
        subject: subject.to_string(),
        body_html: Some(html_body.clone()),
        body_text: Some(text_body.clone()),
        created_at: chrono::Utc::now(),
    });

    let email = Message::builder()
        .from(sender)
        .to(recipient.parse()?)
        .subject(subject)
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    lettre::message::SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(text_body),
                )
                .singlepart(
                    lettre::message::SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html_body),
                ),
        )?;

    let timer = crate::metrics::OperationTimer::start(
        "erno_email_send_duration_seconds",
        "erno_email_send_total",
        "template",
        meta.template.clone().unwrap_or_else(|| "none".to_string()),
    );
    let send_result = app
        .mailer
        .send(email)
        .await
        .map_err(|e| EmailError::MailerError(e.to_string()));
    timer.finish(&send_result);

    persist_email_message(app, recipient, &from, subject, &meta, &send_result).await;

    send_result
}

async fn persist_email_message<ExtraConfig>(
    app: &App<ExtraConfig>,
    recipient: &str,
    from: &str,
    subject: &str,
    meta: &EmailMeta,
    send_result: &Result<(), EmailError>,
) {
    let now = Utc::now().naive_utc();
    let (status, error, sent_at) = match send_result {
        Ok(()) => ("sent", None, Some(now)),
        Err(e) => ("failed", Some(e.to_string()), None),
    };

    let row = email_message::ActiveModel {
        id: Set(Uuid::new_v4()),
        to: Set(recipient.to_string()),
        from: Set(from.to_string()),
        subject: Set(subject.to_string()),
        template: Set(meta.template.clone()),
        user_id: Set(meta.user_id),
        job_id: Set(meta.job_id),
        status: Set(status.to_string()),
        error: Set(error),
        sent_at: Set(sent_at),
        created_at: Set(now),
    };

    if let Err(e) = row.insert(&app.db).await {
        tracing::error!(error = %e, to = recipient, "failed to persist email_messages row");
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    use super::*;
    use crate::{
        app::App,
        database::migrations::Migrator,
        jobs::send_verification_email_job::{SendVerificationEmailArgs, SendVerificationEmailJob},
        tests::setup_test::{setup_test, test_boot},
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

    #[tokio::test]
    async fn send_html_email_inserts_sent_outbox_row() {
        let t = setup_test::<Migrator, _>(test_boot(test_router), no_fixtures).await;
        let app = crate::app::App {
            config: t.config.clone(),
            environment: t.environment,
            db: t.db.clone(),
            mailer: t.mailer.clone(),
            job_queue: t.job_queue.clone(),
            sync_queue: crate::sync::queue::SyncQueue::mock(),
            sync_registry: std::sync::Arc::new(crate::sync::registry::SyncRegistry::new()),
            rate_limit_state: crate::rate_limiting::RateLimitState::new(
                t.config.rate_limiting.clone(),
            ),
            websocket_connections: crate::websocket::connections::Connections::new(),
            storage: crate::storage::FileStorage::mock(),
            prometheus_handle: crate::metrics::setup_metrics(),
            metrics_collectors: std::sync::Arc::new(
                crate::metrics::collector::CollectorRegistry::default(),
            ),
            job_failure_handler: None,
            user_data_deleter: None,
            error_reporter: crate::error_reporting::reporter::ErrorReporter::disabled(),
        };

        send_html_email(&app, "outbox@example.com", "Hello", "<p>Hi</p>".into())
            .await
            .unwrap();

        let rows = email_message::Entity::find()
            .filter(email_message::Column::To.eq("outbox@example.com"))
            .all(&t.db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "sent");
        assert_eq!(rows[0].subject, "Hello");
        assert!(rows[0].sent_at.is_some());
        assert!(rows[0].error.is_none());
    }

    #[tokio::test]
    async fn verification_job_records_template_and_user() {
        use sea_orm::{ActiveModelTrait, Set};

        use crate::{database::models::user, password::hash_password};

        let t = setup_test::<Migrator, _>(test_boot(test_router), no_fixtures).await;
        let u = user::ActiveModel {
            email: Set("verify-outbox@example.com".to_string()),
            password_hash: Set(Some(hash_password("password123").unwrap())),
            ..Default::default()
        }
        .insert(&t.db)
        .await
        .unwrap();

        t.execute_job::<SendVerificationEmailJob>(SendVerificationEmailArgs {
            user_id: u.id,
            email: u.email.clone(),
            raw_token: "token".to_string(),
        })
        .await
        .unwrap();

        let row = email_message::Entity::find()
            .filter(email_message::Column::To.eq("verify-outbox@example.com"))
            .one(&t.db)
            .await
            .unwrap()
            .expect("outbox row");
        assert_eq!(row.template.as_deref(), Some("verification"));
        assert_eq!(row.user_id, Some(u.id));
        assert_eq!(row.status, "sent");
    }
}
