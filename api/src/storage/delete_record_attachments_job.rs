//! Generic background job that removes **all** file attachments (and any
//! orphaned blobs) for an arbitrary record. Apps enqueue this from their
//! [`UserDataDeleter`](crate::account::UserDataDeleter) implementation — one
//! per app-owned record — so files attached to app records are wiped during
//! account deletion too:
//!
//! ```rust,ignore
//! job_queue
//!     .enqueue_by_name(
//!         txn,
//!         delete_record_attachments_job::JOB_NAME,
//!         serde_json::json!({ "record_type": "post", "record_id": post_id }),
//!     )
//!     .await?;
//! ```

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::App,
    jobs::{Job, JobError},
};

/// Registered job-type name.
pub const JOB_NAME: &str = "delete_record_attachments";

pub struct DeleteRecordAttachmentsJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteRecordAttachmentsArgs {
    pub record_type: String,
    pub record_id: Uuid,
}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig>
    for DeleteRecordAttachmentsJob<ExtraConfig>
{
    type Arguments = DeleteRecordAttachmentsArgs;

    fn name() -> &'static str {
        JOB_NAME
    }

    async fn execute(app: &App<ExtraConfig>, args: Self::Arguments) -> Result<(), JobError> {
        app.storage
            .detach_all_for_record(&app.db, args.record_type, args.record_id)
            .await
            .map_err(|e| JobError::TryAgainLater(e.to_string()))
    }

    async fn on_permanent_failure(_app: &App<ExtraConfig>, args: &serde_json::Value, error: &str) {
        tracing::error!(
            "Account deletion: failed to delete attachments for {} {} after retries: {error}",
            args.get("record_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
            args.get("record_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
        );
    }
}
