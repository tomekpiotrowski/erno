//! Background job that deletes a user's Stripe customer object during account
//! deletion. Enqueued by [`purge_user_account`](crate::account::purge_user_account)
//! with the customer id captured before the local row is removed. Deleting the
//! customer removes the PII (email, name, payment methods) held on Stripe's
//! side; Stripe retains invoices for its own tax obligations regardless.

use serde::{Deserialize, Serialize};
use stripe::Client;

use crate::{
    app::App,
    jobs::{Job, JobError},
};

/// Registered job-type name.
pub const JOB_NAME: &str = "delete_stripe_customer";

pub struct DeleteStripeCustomerJob<ExtraConfig = ()>(std::marker::PhantomData<ExtraConfig>);

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteStripeCustomerArgs {
    pub stripe_customer_id: String,
}

impl<ExtraConfig: Clone + Send + Sync + 'static> Job<ExtraConfig>
    for DeleteStripeCustomerJob<ExtraConfig>
{
    type Arguments = DeleteStripeCustomerArgs;

    fn name() -> &'static str {
        JOB_NAME
    }

    async fn execute(app: &App<ExtraConfig>, args: Self::Arguments) -> Result<(), JobError> {
        let Some(stripe_config) = &app.config.stripe else {
            // Stripe isn't configured — nothing to delete.
            return Ok(());
        };

        let customer_id = args
            .stripe_customer_id
            .parse::<stripe::CustomerId>()
            .map_err(|e| JobError::FailPermanently(format!("invalid customer id: {e}")))?;

        let client = Client::new(&stripe_config.secret_key);

        match stripe::Customer::delete(&client, &customer_id).await {
            Ok(_) => Ok(()),
            // 404 = already deleted on Stripe's side; treat as done.
            Err(stripe::StripeError::Stripe(req)) if req.http_status == 404 => {
                tracing::info!("Stripe customer {customer_id} already gone; nothing to delete");
                Ok(())
            }
            Err(e) => Err(JobError::TryAgainLater(format!(
                "stripe customer delete failed: {e}"
            ))),
        }
    }

    async fn on_permanent_failure(_app: &App<ExtraConfig>, args: &serde_json::Value, error: &str) {
        tracing::error!(
            "Account deletion: failed to delete Stripe customer {} after retries — \
             their PII and payment methods remain on Stripe: {error}",
            args.get("stripe_customer_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
        );
    }
}
