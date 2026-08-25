use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use opentelemetry::global;
use std::time::Instant;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::tracing_otel::{HeaderExtractor, HeaderInjector};

pub async fn metrics_middleware(req: Request, next: Next) -> Response {
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());
    let method = req.method().as_str().to_owned();
    let parent = global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(req.headers()))
    });

    let span_name = format!("{method} {path}");
    let span = tracing::info_span!(
        "http.server",
        otel.kind = "server",
        otel.name = span_name.as_str(),
        http.request.method = method.as_str(),
        http.route = path.as_str(),
        http.response.status_code = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    );
    let _ = span.set_parent(parent);

    async move {
        metrics::gauge!("http_requests_in_flight").increment(1.0);
        let start = Instant::now();

        let mut response = next.run(req).await;

        metrics::gauge!("http_requests_in_flight").decrement(1.0);
        let duration = start.elapsed().as_secs_f64();
        let status_code = response.status().as_u16();
        let status = status_code.to_string();

        tracing::Span::current().record("http.response.status_code", status_code);
        tracing::Span::current().record(
            "otel.status_code",
            if status_code >= 500 { "ERROR" } else { "OK" },
        );

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

        let context = tracing::Span::current().context();
        global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&context, &mut HeaderInjector(response.headers_mut()));
        });

        response
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
    use tower::ServiceExt;
    use tracing_subscriber::layer::SubscriberExt;

    fn install_tracer() -> (
        InMemorySpanExporter,
        SdkTracerProvider,
        tracing::subscriber::DefaultGuard,
    ) {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = provider.tracer("test");
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
        let guard = tracing::subscriber::set_default(subscriber);
        global::set_text_map_propagator(TraceContextPropagator::new());
        (exporter, provider, guard)
    }

    #[tokio::test]
    async fn http_span_uses_the_matched_route() {
        let (exporter, provider, _guard) = install_tracer();
        let app = Router::new()
            .route("/widgets/{id}", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(metrics_middleware));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/widgets/abc")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        provider.force_flush().unwrap();
        let spans = exporter.get_finished_spans().unwrap();
        let http = spans
            .iter()
            .find(|s| s.name.as_ref() == "GET /widgets/{id}" || s.name.as_ref() == "http.server")
            .unwrap_or_else(|| panic!("http span, got {spans:?}"));
        let route = http
            .attributes
            .iter()
            .find(|kv| kv.key.as_str() == "http.route")
            .map(|kv| kv.value.to_string());
        assert_eq!(route.as_deref(), Some("/widgets/{id}"));
    }

    #[tokio::test]
    async fn incoming_traceparent_becomes_the_parent() {
        let (exporter, provider, _guard) = install_tracer();
        let app = Router::new()
            .route("/widgets/{id}", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(metrics_middleware));
        let trace_id = "4bf92f3577b34da6a3ce929d0e0e4736";
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/widgets/abc")
                    .header("traceparent", format!("00-{trace_id}-00f067aa0ba902b7-01"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        provider.force_flush().unwrap();
        let spans = exporter.get_finished_spans().unwrap();
        let http = spans
            .iter()
            .find(|s| s.name.as_ref().contains("widgets") || s.name.as_ref() == "http.server")
            .unwrap_or_else(|| panic!("http span, got {spans:?}"));
        assert_eq!(http.span_context.trace_id().to_string(), trace_id);
    }
}
