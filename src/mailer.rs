use std::{
    fmt::{self, Debug},
    sync::{Arc, Mutex},
};

use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Mock transport that captures sent emails for testing.
///
/// Provides a simple in-memory store for emails sent during tests,
/// allowing verification without actually sending emails.
#[derive(Clone)]
pub struct MockTransport {
    messages: Arc<Mutex<Vec<Message>>>,
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl MockTransport {
    /// Create a new mock transport
    pub fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Store a message (called internally by send)
    fn store_message(&self, message: Message) {
        self.messages.lock().unwrap().push(message);
    }

    /// Get all sent messages
    pub fn messages(&self) -> Vec<Message> {
        self.messages.lock().unwrap().clone()
    }

    /// Clear all sent messages
    pub fn clear(&self) {
        self.messages.lock().unwrap().clear();
    }
}

/// Mailer that can be either a real SMTP transport or a mock for testing.
///
/// The mock variant captures sent emails in memory, allowing tests to verify
/// that emails were sent without actually sending them.
#[derive(Clone)]
pub enum Mailer {
    /// Real SMTP transport for production use
    Smtp(AsyncSmtpTransport<Tokio1Executor>),
    /// Mock transport that captures emails for testing
    Mock(MockTransport),
}

impl Debug for Mailer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Smtp(_) => f.debug_tuple("Mailer::Smtp").finish(),
            Self::Mock(_) => f.debug_tuple("Mailer::Mock").finish(),
        }
    }
}

impl Mailer {
    /// Create a new mock mailer for testing
    pub fn mock() -> Self {
        Self::Mock(MockTransport::new())
    }

    /// Create a new SMTP mailer for production
    pub fn smtp(transport: AsyncSmtpTransport<Tokio1Executor>) -> Self {
        Self::Smtp(transport)
    }

    /// Send an email. For mock transport, stores the message for later inspection.
    pub async fn send(
        &self,
        message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match self {
            Self::Smtp(transport) => {
                transport.send(message).await?;
                Ok(())
            }
            Self::Mock(mock) => {
                // Store the full message for inspection
                mock.store_message(message);
                Ok(())
            }
        }
    }

    /// Get sent emails (only available for mock mailer)
    ///
    /// Returns None if this is a real SMTP mailer.
    pub fn messages(&self) -> Option<Vec<Message>> {
        match self {
            Self::Mock(transport) => Some(transport.messages()),
            Self::Smtp(_) => None,
        }
    }

    /// Clear sent emails (only available for mock mailer)
    pub fn clear_messages(&self) {
        if let Self::Mock(transport) = self {
            transport.clear();
        }
    }
}
