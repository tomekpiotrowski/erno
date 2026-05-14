use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};

use crate::{app::App, auth::current_user::CurrentUser};

/// Confirms the current user has an active subscription.
///
/// Reads `subscription_plan` and `subscription_type` cached on the user row —
/// no extra DB query in the happy path. Returns 402 Payment Required if the
/// user has no active subscription.
///
/// # Example
///
/// ```rust,ignore
/// async fn protected_endpoint(
///     _sub: ActiveSubscription,
///     current_user: CurrentUser,
/// ) -> impl IntoResponse { ... }
/// ```
#[derive(Debug, Clone)]
pub struct ActiveSubscription {
    pub plan: String,
    pub subscription_type: String,
}

pub struct PaymentRequired;

impl IntoResponse for PaymentRequired {
    fn into_response(self) -> Response {
        (StatusCode::PAYMENT_REQUIRED, "Payment required").into_response()
    }
}

impl<ExtraConfig> FromRequestParts<App<ExtraConfig>> for ActiveSubscription
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &App<ExtraConfig>,
    ) -> Result<Self, Self::Rejection> {
        let current_user = CurrentUser::<()>::from_request_parts(parts, state)
            .await
            .map_err(IntoResponse::into_response)?;

        match (&current_user.subscription_plan, &current_user.subscription_type) {
            (Some(plan), Some(sub_type)) => Ok(ActiveSubscription {
                plan: plan.clone(),
                subscription_type: sub_type.clone(),
            }),
            _ => Err(PaymentRequired.into_response()),
        }
    }
}
