use lettre::{
    message::{header::ContentType, MultiPart},
    Message,
};
use thiserror::Error;

use crate::{app::App, jobs::JobError};

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
            EmailError::TemplateError(e) => JobError::TryAgainLater(e),
            EmailError::MailerError(e) => JobError::TryAgainLater(e),
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for EmailError {
    fn from(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        EmailError::MailerError(error.to_string())
    }
}

pub async fn send_html_email(
    app: &App,
    recipient: &str,
    subject: &str,
    body: String,
) -> Result<(), EmailError> {
    let sender = match &app.config.email {
        crate::config::EmailConfig::Smtp { sender, .. } => sender.clone(),
        crate::config::EmailConfig::Mock => {
            // For mock, use a placeholder sender
            "noreply@example.com".parse().expect("Invalid mock sender")
        }
    };

    let email = Message::builder()
        .from(sender)
        .to(recipient.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(body)?;

    app.mailer
        .send(email)
        .await
        .map_err(|e| EmailError::MailerError(e.to_string()))?;

    Ok(())
}

/// Sends a multipart email with both plain text and HTML versions.
///
/// This is the preferred method for sending emails as it provides better
/// accessibility and compatibility. Email clients will automatically choose
/// the best format for the user.
pub async fn send_multipart_email(
    app: &App,
    recipient: &str,
    subject: &str,
    text_body: String,
    html_body: String,
) -> Result<(), EmailError> {
    let sender = match &app.config.email {
        crate::config::EmailConfig::Smtp { sender, .. } => sender.clone(),
        crate::config::EmailConfig::Mock => {
            // For mock, use a placeholder sender
            "noreply@example.com".parse().expect("Invalid mock sender")
        }
    };

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

    app.mailer
        .send(email)
        .await
        .map_err(|e| EmailError::MailerError(e.to_string()))?;

    Ok(())
}
