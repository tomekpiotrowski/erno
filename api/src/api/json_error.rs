use axum::{
    extract::rejection::JsonRejection, http::StatusCode, response::IntoResponse,
    response::Response, Json,
};
use validator::ValidationErrors;

#[derive(Debug, thiserror::Error)]
pub enum JsonError {
    #[error("Invalid JSON format")]
    InvalidJson(#[from] JsonRejection),
    #[error("Validation error")]
    ValidationError(ValidationErrors),
}

impl IntoResponse for JsonError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidJson(_) => {
                (StatusCode::BAD_REQUEST, "Invalid JSON format").into_response()
            }
            Self::ValidationError(errors) => {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!(errors))).into_response()
            }
        }
    }
}
