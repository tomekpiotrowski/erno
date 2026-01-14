use axum::{
    extract::{FromRequest, Request},
    Json,
};
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::api::json_error::JsonError;

/// An extractor that deserializes JSON and validates it using the validator crate
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidatedJson<T>(pub T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = JsonError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // First, extract JSON
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(JsonError::InvalidJson)?;

        // Then validate
        value.validate().map_err(JsonError::ValidationError)?;

        Ok(Self(value))
    }
}
