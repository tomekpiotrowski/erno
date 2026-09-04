//! Push metrics to the collector over OTLP — the sending half of "metrics are
//! pushed, not scraped".
//!
//! There is deliberately no second metrics recorder: the `metrics` crate
//! allows one global, and the Prometheus recorder already aggregates every
//! counter, gauge and histogram with the right buckets. This module renders
//! that recorder's exposition text on an interval, computes **deltas** against
//! the previous snapshot, and POSTs an OTLP protobuf batch to
//! `{endpoint}/v1/metrics` with the same bearer the trace and log exporters
//! use.
//!
//! Delta temporality is not a preference but a contract: the collector's
//! rollups fold delta counters and histograms with plain sums, and it refuses
//! cumulative histograms outright. Deltas also make process restarts a
//! non-event — the first snapshot after a restart simply becomes the new
//! baseline (a counter that shrank is a reset, and its current value is the
//! delta).
//!
//! Parsing our own exposition text sounds odd until the alternatives are
//! priced: a fanout recorder means re-implementing histogram aggregation, and
//! scraping stays the failure mode this replaces. The format is stable, ours,
//! and covered by tests.

use std::collections::BTreeMap;
use std::time::Duration;

use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};
use opentelemetry_proto::tonic::metrics::v1 as pb;
use prost::Message as _;

use crate::config::OtelConfig;

/// One series: metric name plus sorted label pairs.
pub type SeriesKey = (String, Vec<(String, String)>);

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Histogram {
    /// Upper bounds, excluding `+Inf`.
    pub bounds: Vec<f64>,
    /// Cumulative counts per bound, Prometheus-style (`le` semantics).
    pub cumulative: Vec<u64>,
    pub count: u64,
    pub sum: f64,
}

/// Everything one render of the exposition text says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub counters: BTreeMap<SeriesKey, f64>,
    pub gauges: BTreeMap<SeriesKey, f64>,
    pub histograms: BTreeMap<SeriesKey, Histogram>,
}

/// Parse Prometheus exposition text. Pure; unknown types and malformed lines
/// are skipped — a partial snapshot beats no snapshot.
#[must_use]
pub fn parse_exposition(text: &str) -> Snapshot {
    let mut snapshot = Snapshot::default();
    let mut types: BTreeMap<String, String> = BTreeMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# TYPE ") {
            if let Some((name, kind)) = rest.rsplit_once(' ') {
                types.insert(name.to_string(), kind.to_string());
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some((name, labels, value)) = parse_sample(line) else {
            continue;
        };

        // Histogram component samples reference the family name.
        if let Some(family) = name.strip_suffix("_bucket") {
            if types.get(family).map(String::as_str) == Some("histogram") {
                let le = labels
                    .iter()
                    .find(|(k, _)| k == "le")
                    .map(|(_, v)| v.clone());
                let key = series_key(family, &labels, true);
                let entry = snapshot.histograms.entry(key).or_default();
                match le.as_deref() {
                    Some("+Inf") | None => {}
                    Some(bound) => {
                        if let Ok(bound) = bound.parse::<f64>() {
                            entry.bounds.push(bound);
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            entry.cumulative.push(value.max(0.0) as u64);
                        }
                    }
                }
                continue;
            }
        }
        if let Some(family) = name.strip_suffix("_sum") {
            if types.get(family).map(String::as_str) == Some("histogram") {
                snapshot
                    .histograms
                    .entry(series_key(family, &labels, false))
                    .or_default()
                    .sum = value;
                continue;
            }
        }
        if let Some(family) = name.strip_suffix("_count") {
            if types.get(family).map(String::as_str) == Some("histogram") {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let count = value.max(0.0) as u64;
                snapshot
                    .histograms
                    .entry(series_key(family, &labels, false))
                    .or_default()
                    .count = count;
                continue;
            }
        }

        match types.get(name).map(String::as_str) {
            Some("counter") => {
                snapshot
                    .counters
                    .insert(series_key(name, &labels, false), value);
            }
            Some("gauge") => {
                snapshot
                    .gauges
                    .insert(series_key(name, &labels, false), value);
            }
            // Summaries (quantile lines) and anything unknown are skipped:
            // precomputed quantiles cannot be aggregated downstream.
            _ => {}
        }
    }
    snapshot
}

fn series_key(name: &str, labels: &[(String, String)], drop_le: bool) -> SeriesKey {
    let mut labels: Vec<(String, String)> = labels
        .iter()
        .filter(|(k, _)| !(drop_le && k == "le"))
        .cloned()
        .collect();
    labels.sort();
    (name.to_string(), labels)
}

/// One parsed sample line.
type Sample<'a> = (&'a str, Vec<(String, String)>, f64);

/// `name{a="x",b="y"} 12.5` → (name, labels, value). Handles escaped quotes.
fn parse_sample(line: &str) -> Option<Sample<'_>> {
    let (head, value) = match line.find('{') {
        Some(brace) => {
            let close = find_closing_brace(line, brace)?;
            (&line[..close + 1], line[close + 1..].trim())
        }
        None => {
            let space = line.find(' ')?;
            (&line[..space], line[space + 1..].trim())
        }
    };
    // Exposition may append a timestamp; the value is the first token.
    let value: f64 = value.split_whitespace().next()?.parse().ok()?;
    match head.find('{') {
        None => Some((head, Vec::new(), value)),
        Some(brace) => {
            let name = &head[..brace];
            let labels = parse_labels(&head[brace + 1..head.len() - 1])?;
            Some((name, labels, value))
        }
    }
}

fn find_closing_brace(line: &str, open: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_quotes = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(open + 1) {
        match b {
            _ if escaped => escaped = false,
            b'\\' => escaped = true,
            b'"' => in_quotes = !in_quotes,
            b'}' if !in_quotes => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_labels(body: &str) -> Option<Vec<(String, String)>> {
    let mut labels = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let eq = rest.find("=\"")?;
        let key = rest[..eq].trim_start_matches(',').trim().to_string();
        let mut value = String::new();
        let mut chars = rest[eq + 2..].char_indices();
        let mut consumed = None;
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some((_, next)) = chars.next() {
                        value.push(match next {
                            'n' => '\n',
                            c => c,
                        });
                    }
                }
                '"' => {
                    consumed = Some(eq + 2 + i + 1);
                    break;
                }
                c => value.push(c),
            }
        }
        labels.push((key, value));
        rest = rest[consumed?..].trim_start_matches(',').trim();
    }
    Some(labels)
}

/// What one interval contributes: deltas for the monotonic shapes, current
/// values for gauges.
#[derive(Debug, Default, PartialEq)]
pub struct DeltaBatch {
    pub counters: Vec<(SeriesKey, f64)>,
    pub gauges: Vec<(SeriesKey, f64)>,
    pub histograms: Vec<(SeriesKey, Histogram)>,
}

/// Diff two snapshots. A counter that shrank is a restarted process: its
/// current value is the delta. A histogram's per-bucket counts come out
/// de-cumulated (OTLP wants per-bucket, `le` semantics are Prometheus's).
#[must_use]
pub fn delta(previous: &Snapshot, current: &Snapshot) -> DeltaBatch {
    let mut batch = DeltaBatch::default();
    for (key, &value) in &current.counters {
        let prev = previous.counters.get(key).copied().unwrap_or(0.0);
        let d = if value < prev { value } else { value - prev };
        if d != 0.0 {
            batch.counters.push((key.clone(), d));
        }
    }
    // Gauges are not deltas; every interval reports the current level.
    for (key, &value) in &current.gauges {
        batch.gauges.push((key.clone(), value));
    }
    for (key, hist) in &current.histograms {
        let empty = Histogram::default();
        let prev = previous.histograms.get(key).unwrap_or(&empty);
        let reset = hist.count < prev.count || prev.bounds != hist.bounds;
        let base = if reset { &empty } else { prev };

        let count = hist.count - base.count;
        if count == 0 {
            continue;
        }
        // De-cumulate within the snapshot, then diff against the base.
        let mut per_bucket = Vec::with_capacity(hist.bounds.len() + 1);
        let mut last_cum = 0u64;
        for (i, &cum) in hist.cumulative.iter().enumerate() {
            let base_cum = base.cumulative.get(i).copied().unwrap_or(0);
            let base_last = if i == 0 {
                0
            } else {
                base.cumulative.get(i - 1).copied().unwrap_or(0)
            };
            let this = (cum - last_cum).saturating_sub(base_cum - base_last);
            per_bucket.push(this);
            last_cum = cum;
        }
        let in_bounds: u64 = per_bucket.iter().sum();
        per_bucket.push(count.saturating_sub(in_bounds));

        batch.histograms.push((
            key.clone(),
            Histogram {
                bounds: hist.bounds.clone(),
                cumulative: per_bucket, // per-bucket now, name kept for reuse
                count,
                sum: hist.sum - base.sum,
            },
        ));
    }
    batch
}

/// The OTLP request body for one interval.
#[must_use]
pub fn to_otlp(
    batch: &DeltaBatch,
    service_name: &str,
    service_version: &str,
    now_unix_nanos: u64,
    interval: Duration,
) -> ExportMetricsServiceRequest {
    let start =
        now_unix_nanos.saturating_sub(u64::try_from(interval.as_nanos()).unwrap_or(u64::MAX));
    let attr = |k: &str, v: &str| KeyValue {
        key: k.to_string(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(v.to_string())),
        }),
    };
    let labels_of =
        |key: &SeriesKey| -> Vec<KeyValue> { key.1.iter().map(|(k, v)| attr(k, v)).collect() };

    let mut metrics = Vec::new();
    for (key, value) in &batch.counters {
        metrics.push(pb::Metric {
            name: key.0.clone(),
            data: Some(pb::metric::Data::Sum(pb::Sum {
                data_points: vec![pb::NumberDataPoint {
                    attributes: labels_of(key),
                    start_time_unix_nano: start,
                    time_unix_nano: now_unix_nanos,
                    value: Some(pb::number_data_point::Value::AsDouble(*value)),
                    ..Default::default()
                }],
                aggregation_temporality: pb::AggregationTemporality::Delta as i32,
                is_monotonic: true,
            })),
            ..Default::default()
        });
    }
    for (key, value) in &batch.gauges {
        metrics.push(pb::Metric {
            name: key.0.clone(),
            data: Some(pb::metric::Data::Gauge(pb::Gauge {
                data_points: vec![pb::NumberDataPoint {
                    attributes: labels_of(key),
                    time_unix_nano: now_unix_nanos,
                    value: Some(pb::number_data_point::Value::AsDouble(*value)),
                    ..Default::default()
                }],
            })),
            ..Default::default()
        });
    }
    for (key, hist) in &batch.histograms {
        metrics.push(pb::Metric {
            name: key.0.clone(),
            data: Some(pb::metric::Data::Histogram(pb::Histogram {
                data_points: vec![pb::HistogramDataPoint {
                    attributes: labels_of(key),
                    start_time_unix_nano: start,
                    time_unix_nano: now_unix_nanos,
                    count: hist.count,
                    sum: Some(hist.sum),
                    bucket_counts: hist.cumulative.clone(),
                    explicit_bounds: hist.bounds.clone(),
                    ..Default::default()
                }],
                aggregation_temporality: pb::AggregationTemporality::Delta as i32,
            })),
            ..Default::default()
        });
    }

    ExportMetricsServiceRequest {
        resource_metrics: vec![pb::ResourceMetrics {
            resource: Some(opentelemetry_proto::tonic::resource::v1::Resource {
                attributes: vec![
                    attr("service.name", service_name),
                    attr("service.version", service_version),
                ],
                ..Default::default()
            }),
            scope_metrics: vec![pb::ScopeMetrics {
                metrics,
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

/// The push loop: render → parse → delta → POST. Spawned by serve when
/// `[tracing.otel]` has an endpoint; failures warn and wait for the next
/// interval — the recorder still holds the totals, so nothing is lost, only
/// late.
pub async fn push_loop(
    handle: metrics_exporter_prometheus::PrometheusHandle,
    config: OtelConfig,
    service_version: String,
) {
    let Some(endpoint) = config.metrics_target() else {
        return;
    };
    let url = format!("{}/v1/metrics", endpoint.trim_end_matches('/'));
    let interval = Duration::from_secs(config.metrics_interval_seconds.max(1));
    let client = reqwest::Client::new();
    let service_name = if config.service_name.trim().is_empty() {
        "erno".to_string()
    } else {
        config.service_name.trim().to_string()
    };

    let mut previous = Snapshot::default();
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let current = parse_exposition(&handle.render());
        let batch = delta(&previous, &current);
        previous = current;
        if batch.counters.is_empty() && batch.gauges.is_empty() && batch.histograms.is_empty() {
            continue;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let now_nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64;
        let body =
            to_otlp(&batch, &service_name, &service_version, now_nanos, interval).encode_to_vec();

        let mut request = client
            .post(&url)
            .header("content-type", "application/x-protobuf")
            .body(body);
        if !config.token.trim().is_empty() {
            request = request.bearer_auth(config.token.trim());
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => {
                tracing::warn!(
                    "otlp metrics push: collector answered {}",
                    response.status()
                );
            }
            Err(e) => {
                tracing::warn!("otlp metrics push failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "\
# TYPE http_requests_total counter\n\
http_requests_total{method=\"GET\",path=\"/api/x\"} 42\n\
# TYPE jobs_pending_count gauge\n\
jobs_pending_count 7\n\
# TYPE http_request_duration_seconds histogram\n\
http_request_duration_seconds_bucket{path=\"/api/x\",le=\"0.1\"} 10\n\
http_request_duration_seconds_bucket{path=\"/api/x\",le=\"0.5\"} 30\n\
http_request_duration_seconds_bucket{path=\"/api/x\",le=\"+Inf\"} 33\n\
http_request_duration_seconds_sum{path=\"/api/x\"} 4.2\n\
http_request_duration_seconds_count{path=\"/api/x\"} 33\n";

    #[test]
    fn the_exposition_parses_into_typed_series() {
        let snapshot = parse_exposition(TEXT);
        let counter_key = (
            "http_requests_total".to_string(),
            vec![
                ("method".to_string(), "GET".to_string()),
                ("path".to_string(), "/api/x".to_string()),
            ],
        );
        assert_eq!(snapshot.counters.get(&counter_key), Some(&42.0));
        assert_eq!(
            snapshot
                .gauges
                .get(&("jobs_pending_count".to_string(), vec![])),
            Some(&7.0)
        );
        let hist = snapshot
            .histograms
            .get(&(
                "http_request_duration_seconds".to_string(),
                vec![("path".to_string(), "/api/x".to_string())],
            ))
            .expect("histogram");
        assert_eq!(hist.bounds, vec![0.1, 0.5]);
        assert_eq!(hist.cumulative, vec![10, 30]);
        assert_eq!(hist.count, 33);
        assert!((hist.sum - 4.2).abs() < 1e-9);
    }

    #[test]
    fn escaped_label_values_survive() {
        let text = "# TYPE x counter\nx{a=\"quo\\\"te\",b=\"line\\nbreak\"} 1\n";
        let snapshot = parse_exposition(text);
        let key = snapshot.counters.keys().next().expect("one series");
        assert_eq!(key.1[0].1, "quo\"te");
        assert_eq!(key.1[1].1, "line\nbreak");
    }

    #[test]
    fn deltas_subtract_and_treat_shrinkage_as_restart() {
        let previous = parse_exposition(TEXT);
        let mut bumped = String::from(TEXT);
        bumped = bumped.replace("} 42", "} 50");
        let current = parse_exposition(&bumped);
        let batch = delta(&previous, &current);
        assert_eq!(batch.counters.len(), 1);
        assert!((batch.counters[0].1 - 8.0).abs() < 1e-9);

        // The process restarted: the counter fell to 3 — 3 is the delta.
        let restarted = parse_exposition(&TEXT.replace("} 42", "} 3"));
        let batch = delta(&previous, &restarted);
        assert!((batch.counters[0].1 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn histogram_deltas_come_out_per_bucket_with_an_overflow() {
        let previous = parse_exposition(TEXT);
        let next_text = TEXT
            .replace("le=\"0.1\"} 10", "le=\"0.1\"} 12")
            .replace("le=\"0.5\"} 30", "le=\"0.5\"} 35")
            .replace("le=\"+Inf\"} 33", "le=\"+Inf\"} 39")
            .replace("_count{path=\"/api/x\"} 33", "_count{path=\"/api/x\"} 39")
            .replace("_sum{path=\"/api/x\"} 4.2", "_sum{path=\"/api/x\"} 6.2");
        let current = parse_exposition(&next_text);
        let batch = delta(&previous, &current);
        assert_eq!(batch.histograms.len(), 1);
        let (_, hist) = &batch.histograms[0];
        // +2 in <=0.1, +3 in (0.1,0.5], +1 beyond.
        assert_eq!(hist.cumulative, vec![2, 3, 1]);
        assert_eq!(hist.count, 6);
        assert!((hist.sum - 2.0).abs() < 1e-9);
        assert_eq!(hist.bounds, vec![0.1, 0.5]);
    }

    #[test]
    fn an_unchanged_interval_sends_gauges_but_no_counters() {
        let snapshot = parse_exposition(TEXT);
        let batch = delta(&snapshot, &snapshot.clone());
        assert!(batch.counters.is_empty());
        assert!(batch.histograms.is_empty());
        assert_eq!(batch.gauges.len(), 1, "levels are re-reported");
    }

    #[test]
    fn the_otlp_body_is_delta_temporality_throughout() {
        let previous = Snapshot::default();
        let current = parse_exposition(TEXT);
        let batch = delta(&previous, &current);
        let request = to_otlp(
            &batch,
            "erno",
            "1.0.0",
            1_000_000_000,
            Duration::from_secs(15),
        );
        for metric in &request.resource_metrics[0].scope_metrics[0].metrics {
            match &metric.data {
                Some(pb::metric::Data::Sum(sum)) => {
                    assert_eq!(
                        sum.aggregation_temporality,
                        pb::AggregationTemporality::Delta as i32
                    );
                    assert!(sum.is_monotonic);
                }
                Some(pb::metric::Data::Histogram(hist)) => {
                    assert_eq!(
                        hist.aggregation_temporality,
                        pb::AggregationTemporality::Delta as i32
                    );
                }
                Some(pb::metric::Data::Gauge(_)) => {}
                other => panic!("unexpected shape: {other:?}"),
            }
        }
    }
}
