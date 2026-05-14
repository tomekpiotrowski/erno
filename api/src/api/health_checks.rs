use axum_macros::debug_handler;

#[debug_handler]
pub async fn ok() -> &'static str {
    "OK"
}
