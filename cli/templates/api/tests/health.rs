//! Canonical request spec. Add one file per resource family under tests/.
//! Guide: https://docs — Erno / API / Testing

mod common;

#[tokio::test]
async fn health_is_public() {
    let t = common::setup().await;
    let response = t.server.get("/api/health").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.text(), "OK");
}
