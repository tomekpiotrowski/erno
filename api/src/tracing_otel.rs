//! Optional OpenTelemetry export, next to tracing-subscriber.
//!
//! Docs: docs/src/content/docs/api/telemetry.md
//!
//! Empty `[tracing.otel] endpoint` leaves this inert: no exporter, no extra
//! spans. Unreachable Tempo must not take the process down — the batch
//! exporter drops.

use std::collections::HashMap;
use std::sync::OnceLock;

use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::{SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider, Tracer};
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::registry::LookupSpan;

use crate::config::OtelConfig;

static TRACER_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

const DEFAULT_SERVICE_NAME: &str = "erno";

/// A tracing layer that records spans on the process tracer provider.
///
/// `None` when `[tracing.otel] endpoint` is empty or the exporter could not
/// be built — the subscriber is then fmt + error-capture only.
pub fn trace_layer<S>(config: &OtelConfig) -> Option<OpenTelemetryLayer<S, Tracer>>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let provider = provider(config)?;
    let tracer = provider.tracer(service_name(config));
    Some(tracing_opentelemetry::layer().with_tracer(tracer))
}

fn provider(config: &OtelConfig) -> Option<&'static SdkTracerProvider> {
    if !config.traces_enabled() {
        return None;
    }
    if let Some(existing) = TRACER_PROVIDER.get() {
        return Some(existing);
    }
    match build_provider(config) {
        Ok(built) => {
            let _ = TRACER_PROVIDER.set(built);
            let provider = TRACER_PROVIDER.get()?;
            global::set_tracer_provider(provider.clone());
            global::set_text_map_propagator(TraceContextPropagator::new());
            Some(provider)
        }
        Err(e) => {
            // tracing is not initialised yet — this runs as the subscriber is
            // being assembled — so a warn would vanish. Same reason the
            // error-reporter path uses eprintln.
            eprintln!("opentelemetry: could not install the trace exporter: {e}; traces are off");
            None
        }
    }
}

fn build_provider(
    config: &OtelConfig,
) -> Result<SdkTracerProvider, Box<dyn std::error::Error + Send + Sync>> {
    let mut exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(config.endpoint.trim());
    if !config.token.trim().is_empty() {
        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", config.token.trim()),
        );
        exporter = exporter.with_headers(headers);
    }
    let exporter = exporter.build()?;

    Ok(SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_sampler(sampler(config.sample_ratio))
        .with_resource(resource(config))
        .build())
}

fn sampler(ratio: f64) -> Sampler {
    if ratio >= 1.0 {
        Sampler::AlwaysOn
    } else if ratio <= 0.0 {
        Sampler::AlwaysOff
    } else {
        Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(ratio)))
    }
}

fn service_name(config: &OtelConfig) -> String {
    let name = config.service_name.trim();
    if name.is_empty() {
        DEFAULT_SERVICE_NAME.to_string()
    } else {
        name.to_string()
    }
}

fn resource(config: &OtelConfig) -> Resource {
    let mut attrs = vec![KeyValue::new(SERVICE_NAME, service_name(config))];
    if let Ok(v) = std::env::var("ERNO_VERSION") {
        attrs.push(KeyValue::new(SERVICE_VERSION, v));
    }
    if let Ok(env) = std::env::var("APP_ENVIRONMENT") {
        attrs.push(KeyValue::new("deployment.environment", env));
    }
    Resource::builder().with_attributes(attrs).build()
}

/// W3C `traceparent` extractor over an http header map.
pub struct HeaderExtractor<'a>(pub &'a axum::http::HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// W3C `traceparent` injector over a mutable http header map.
pub struct HeaderInjector<'a>(pub &'a mut axum::http::HeaderMap);

impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(name) = axum::http::HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(value) = axum::http::HeaderValue::from_str(&value) {
                self.0.insert(name, value);
            }
        }
    }
}

/// Flush outstanding spans. No-op when export was never installed.
pub fn flush() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        let _ = provider.force_flush();
    }
}

/// Flush and shut the provider down. Called at the end of process drain.
pub fn shutdown() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_sdk::trace::InMemorySpanExporter;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn empty_endpoint_installs_no_layer() {
        let config = OtelConfig::default();
        assert!(!config.traces_enabled());
        assert!(trace_layer::<tracing_subscriber::Registry>(&config).is_none());
        assert!(TRACER_PROVIDER.get().is_none());
    }

    #[test]
    fn otel_config_deserializes_under_tracing() {
        let parsed: crate::config::TracingConfig = toml::from_str(
            r#"
            log_level = "info"
            [otel]
            endpoint = "http://127.0.0.1:4318"
            sample_ratio = 1.0
            service_name = "erno-api"
            "#,
        )
        .expect("toml");
        assert_eq!(parsed.log_level, "info");
        assert_eq!(parsed.otel.endpoint, "http://127.0.0.1:4318");
        assert_eq!(parsed.otel.sample_ratio, 1.0);
        assert_eq!(parsed.otel.service_name, "erno-api");
        assert!(parsed.otel.traces_enabled());
    }

    #[test]
    fn missing_otel_table_stays_off() {
        let parsed: crate::config::TracingConfig =
            toml::from_str(r#"log_level = "debug""#).expect("toml");
        assert!(!parsed.otel.traces_enabled());
        assert_eq!(parsed.otel.sample_ratio, 0.1);
    }

    #[test]
    fn a_span_is_recorded_on_an_in_memory_exporter() {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = provider.tracer("test");
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("widget");
            let _guard = span.enter();
        });

        provider.force_flush().expect("flush");
        let spans = exporter.get_finished_spans().expect("spans");
        assert!(
            spans.iter().any(|s| s.name == "widget"),
            "expected a widget span, got {spans:?}"
        );
    }

    #[test]
    fn logs_target_inherits_endpoint_when_log_level_is_set() {
        let mut config = OtelConfig {
            endpoint: "http://127.0.0.1:4318".into(),
            log_level: "warn".into(),
            ..OtelConfig::default()
        };
        assert_eq!(config.logs_target(), Some("http://127.0.0.1:4318"));
        config.logs_endpoint = "http://127.0.0.1:3100/otlp".into();
        assert_eq!(config.logs_target(), Some("http://127.0.0.1:3100/otlp"));
        config.log_level.clear();
        assert_eq!(config.logs_target(), None);
    }
}
