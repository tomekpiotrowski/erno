use std::{
    fmt::{self, Debug},
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct MockEmailRecord {
    pub id: Uuid,
    pub to: String,
    pub from: String,
    pub subject: String,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct MockTransport {
    records: Arc<Mutex<Vec<MockEmailRecord>>>,
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            records: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn store_record(&self, record: MockEmailRecord) {
        self.records.lock().unwrap().push(record);
    }

    pub fn records(&self) -> Vec<MockEmailRecord> {
        self.records.lock().unwrap().clone()
    }

    pub fn remove_record(&self, id: Uuid) -> bool {
        let mut records = self.records.lock().unwrap();
        let len_before = records.len();
        records.retain(|r| r.id != id);
        records.len() < len_before
    }

    pub fn clear(&self) {
        self.records.lock().unwrap().clear();
    }
}

#[derive(Clone)]
pub enum Mailer {
    Smtp(AsyncSmtpTransport<Tokio1Executor>),
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
    pub fn mock() -> Self {
        Self::Mock(MockTransport::new())
    }

    pub fn smtp(transport: AsyncSmtpTransport<Tokio1Executor>) -> Self {
        Self::Smtp(transport)
    }

    pub async fn send(
        &self,
        message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match self {
            Self::Smtp(transport) => {
                transport.send(message).await?;
                Ok(())
            }
            Self::Mock(_) => Ok(()),
        }
    }

    pub fn store_record(&self, record: MockEmailRecord) {
        if let Self::Mock(transport) = self {
            transport.store_record(record);
        }
    }

    pub fn records(&self) -> Option<Vec<MockEmailRecord>> {
        match self {
            Self::Mock(transport) => Some(transport.records()),
            Self::Smtp(_) => None,
        }
    }

    pub fn remove_record(&self, id: Uuid) -> bool {
        match self {
            Self::Mock(transport) => transport.remove_record(id),
            Self::Smtp(_) => false,
        }
    }

    pub fn clear_messages(&self) {
        if let Self::Mock(transport) = self {
            transport.clear();
        }
    }
}
