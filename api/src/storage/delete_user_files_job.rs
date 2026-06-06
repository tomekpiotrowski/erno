//! Background job that removes all files attached to a user during account
//! deletion. Enqueued by [`purge_user_account`](crate::account::purge_user_account).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::App,
    jobs::{Job, JobError},
};

/// Registered job-type name.
pub const JOB_NAME: &str = "delete_user_files";

pub struct DeleteUserFilesJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteUserFilesArgs {
    pub user_id: Uuid,
}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig> for DeleteUserFilesJob<ExtraConfig> {
    type Arguments = DeleteUserFilesArgs;

    fn name() -> &'static str {
        JOB_NAME
    }

    async fn execute(app: &App<ExtraConfig>, args: Self::Arguments) -> Result<(), JobError> {
        app.storage
            .detach_all_for_record(&app.db, "user", args.user_id)
            .await
            .map_err(|e| JobError::TryAgainLater(e.to_string()))
    }

    async fn on_permanent_failure(_app: &App<ExtraConfig>, args: &serde_json::Value, error: &str) {
        tracing::error!(
            "Account deletion: failed to delete files for user {} after retries: {error}",
            args.get("user_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
        );
    }
}
