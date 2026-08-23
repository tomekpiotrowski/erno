//! Background job that anonymises a deleted user's stored error events.
//!
//! Enqueued by [`purge_user_account`](crate::account::purge_user_account).
//!
//! **A job rather than an inline call**, for two reasons. `purge_user_account`
//! runs inside the deletion transaction, and an inline HTTP request would hold
//! that transaction open across the network. And this is an erasure obligation:
//! if the collector is down, "try again later" is the only acceptable answer —
//! silently dropping it is not.
//!
//! **Anonymise, not delete**: a stack trace, a release and a fingerprint are not
//! personal data, and removing the rows would corrupt `times_seen` and every
//! time series built on them. The collector nulls `user_id` and `user_email`.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::App,
    error_reporting::reporter::sender::{classify_status, Disposition},
    jobs::{Job, JobError},
};

/// Registered job-type name.
pub const JOB_NAME: &str = "anonymize_collector_events";

pub struct AnonymizeCollectorEventsJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct AnonymizeCollectorEventsArgs {
    pub user_id: Uuid,
}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig>
    for AnonymizeCollectorEventsJob<ExtraConfig>
{
    type Arguments = AnonymizeCollectorEventsArgs;

    fn name() -> &'static str {
        JOB_NAME
    }

    async fn execute(app: &App<ExtraConfig>, args: Self::Arguments) -> Result<(), JobError> {
        let config = &app.config.error_reporting;
        // Enqueued unconditionally so the job row is a durable record that
        // erasure was attempted; a deployment with no collector simply has
        // nothing to erase.
        if !config.is_active() {
            return Ok(());
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(
                config.request_timeout_ms.max(1),
            ))
            .build()
            .map_err(|e| JobError::TryAgainLater(e.to_string()))?;

        let response = client
            .delete(config.user_events_endpoint(args.user_id))
            .header("X-Erno-Ingest-Key", &config.ingest_token)
            .send()
            .await
            .map_err(|e| JobError::TryAgainLater(format!("could not reach the collector: {e}")))?;

        let status = response.status().as_u16();
        // A user with no stored events is not a failure — there is nothing to
        // anonymise and the obligation is met.
        if status == 404 {
            return Ok(());
        }
        match classify_status(status) {
            Disposition::Delivered => Ok(()),
            // The payload is the problem, so retrying loops forever.
            Disposition::Discard => Err(JobError::FailPermanently(format!(
                "the collector rejected the anonymise request with {status}"
            ))),
            Disposition::Retry => Err(JobError::TryAgainLater(format!(
                "the collector returned {status}"
            ))),
        }
    }

    async fn on_permanent_failure(_app: &App<ExtraConfig>, args: &serde_json::Value, error: &str) {
        tracing::error!(
            "Account deletion: could not anonymise collector events for user {} after retries: \
             {error}. Those rows still carry the user's id and email.",
            args.get("user_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
        );
    }
}
