use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use std::time::Instant;

pub async fn metrics_middleware(req: Request, next: Next) -> Response {
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());
    let method = req.method().as_str().to_owned();

    metrics::gauge!("http_requests_in_flight").increment(1.0);
    let start = Instant::now();

    let response = next.run(req).await;

    metrics::gauge!("http_requests_in_flight").decrement(1.0);
    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    metrics::counter!("http_requests_total",
        "method" => method.clone(),
        "path" => path.clone(),
        "status" => status.clone(),
    )
    .increment(1);

    metrics::histogram!("http_request_duration_seconds",
        "method" => method,
        "path" => path,
        "status" => status,
    )
    .record(duration);

    response
}
