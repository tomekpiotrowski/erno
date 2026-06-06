//! Background job that cancels a user's Stripe subscription during account
//! deletion. Enqueued by [`purge_user_account`](crate::account::purge_user_account)
//! with the subscription id captured before the local row is removed.

use serde::{Deserialize, Serialize};
use stripe::{CancelSubscription, Client};

use crate::{
    app::App,
    jobs::{Job, JobError},
};

/// Registered job-type name.
pub const JOB_NAME: &str = "cancel_stripe_subscription";

pub struct CancelStripeSubscriptionJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct CancelStripeSubscriptionArgs {
    pub stripe_subscription_id: String,
}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig>
    for CancelStripeSubscriptionJob<ExtraConfig>
{
    type Arguments = CancelStripeSubscriptionArgs;

    fn name() -> &'static str {
        JOB_NAME
    }

    async fn execute(app: &App<ExtraConfig>, args: Self::Arguments) -> Result<(), JobError> {
        let Some(stripe_config) = &app.config.stripe else {
            // Stripe isn't configured — nothing to cancel.
            return Ok(());
        };

        let subscription_id = args
            .stripe_subscription_id
            .parse::<stripe::SubscriptionId>()
            .map_err(|e| JobError::FailPermanently(format!("invalid subscription id: {e}")))?;

        let client = Client::new(&stripe_config.secret_key);

        match stripe::Subscription::cancel(&client, &subscription_id, CancelSubscription::new()).await
        {
            Ok(_) => Ok(()),
            // 404 = already cancelled/deleted on Stripe's side; treat as done.
            Err(stripe::StripeError::Stripe(req)) if req.http_status == 404 => {
                tracing::info!(
                    "Stripe subscription {subscription_id} already gone; nothing to cancel"
                );
                Ok(())
            }
            Err(e) => Err(JobError::TryAgainLater(format!("stripe cancel failed: {e}"))),
        }
    }

    async fn on_permanent_failure(_app: &App<ExtraConfig>, args: &serde_json::Value, error: &str) {
        tracing::error!(
            "Account deletion: failed to cancel Stripe subscription {} after retries — \
             it may still be billing: {error}",
            args.get("stripe_subscription_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
        );
    }
}
