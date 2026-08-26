use std::collections::HashMap;

use reqwest::Client;
use serde::Deserialize;

#[derive(Clone, Debug, Default)]
pub struct PromSnapshot {
    pub req_per_s: f64,
    pub p95_ms: f64,
    pub err_per_s: f64,
    pub pool: f64,
    pub req_hist: Vec<f64>,
    pub p50_hist: Vec<f64>,
    pub p95_hist: Vec<f64>,
    pub routes: Vec<RouteRow>,
}

#[derive(Clone, Debug)]
pub struct RouteRow {
    pub path: String,
    pub calls: String,
    pub ms: String,
}

const Q_RATE: &str = "sum(rate(http_requests_total[1m]))";
const Q_P95: &str =
    "histogram_quantile(0.95, sum by (le) (rate(http_request_duration_seconds_bucket[1m])))";
const Q_P50: &str =
    "histogram_quantile(0.50, sum by (le) (rate(http_request_duration_seconds_bucket[1m])))";
const Q_ERR: &str = "sum(rate(http_requests_total{status=~\"5..\"}[1m]))";
const Q_POOL: &str = "db_pool_connections_total";
const Q_ROUTES: &str = "sum by (path) (rate(http_requests_total[1m]))";
const Q_ROUTE_MS: &str =
    "histogram_quantile(0.95, sum by (le, path) (rate(http_request_duration_seconds_bucket[1m])))";

pub async fn fetch(
    client: &Client,
    base: &str,
    token: Option<&str>,
    prev: &PromSnapshot,
) -> PromSnapshot {
    let (rate, p95, p50, err, pool, route_rows) = tokio::join!(
        instant(client, base, token, Q_RATE),
        instant(client, base, token, Q_P95),
        instant(client, base, token, Q_P50),
        instant(client, base, token, Q_ERR),
        instant(client, base, token, Q_POOL),
        routes(client, base, token),
    );
    let rate = rate.unwrap_or(0.0);
    let p95 = p95.unwrap_or(0.0);
    let p50 = p50.unwrap_or(0.0);
    let err = err.unwrap_or(0.0);
    let pool = pool.unwrap_or(0.0);
    let mut snap = prev.clone();
    push_hist(&mut snap.req_hist, rate);
    push_hist(&mut snap.p50_hist, p50 * 1000.0);
    push_hist(&mut snap.p95_hist, p95 * 1000.0);
    snap.req_per_s = rate;
    snap.p95_ms = p95 * 1000.0;
    snap.err_per_s = err;
    snap.pool = pool;
    snap.routes = route_rows;
    snap
}

async fn routes(client: &Client, base: &str, token: Option<&str>) -> Vec<RouteRow> {
    let calls = vector(client, base, token, Q_ROUTES).await;
    let ms = vector(client, base, token, Q_ROUTE_MS).await;
    let mut out: Vec<RouteRow> = calls
        .into_iter()
        .map(|(path, v)| RouteRow {
            path: path.clone(),
            calls: format!("{v:.1}/s"),
            ms: ms
                .get(&path)
                .map(|s| format!("{:.0}ms", s * 1000.0))
                .unwrap_or_else(|| "—".into()),
        })
        .collect();
    out.sort_by(|a, b| b.calls.cmp(&a.calls));
    out.truncate(6);
    out
}

fn push_hist(hist: &mut Vec<f64>, v: f64) {
    hist.push(v);
    if hist.len() > 24 {
        hist.remove(0);
    }
}

pub fn spark(hist: &[f64]) -> String {
    const BLOCKS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if hist.is_empty() {
        return String::new();
    }
    let max = hist.iter().copied().fold(0.0_f64, f64::max).max(1e-9);
    hist.iter()
        .map(|v| {
            let i = ((v / max) * 7.0).round().clamp(0.0, 7.0) as usize;
            BLOCKS[i]
        })
        .collect()
}

async fn instant(client: &Client, base: &str, token: Option<&str>, query: &str) -> Option<f64> {
    vector(client, base, token, query)
        .await
        .into_iter()
        .next()
        .map(|(_, v)| v)
}

async fn vector(
    client: &Client,
    base: &str,
    token: Option<&str>,
    query: &str,
) -> HashMap<String, f64> {
    let url = format!("{}/api/v1/query", base.trim_end_matches('/'));
    let mut req = client.get(url).query(&[("query", query)]);
    if let Some(t) = token.filter(|s| !s.is_empty()) {
        req = req.bearer_auth(t);
    }
    let Ok(res) = req.send().await else {
        return HashMap::new();
    };
    let Ok(body) = res.json::<PromResponse>().await else {
        return HashMap::new();
    };
    parse_vector(&body)
}

pub fn parse_vector(body: &PromResponse) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for row in &body.data.result {
        let key = row.metric.get("path").cloned().unwrap_or_else(String::new);
        if let Some(v) = row.value.as_ref().and_then(parse_sample) {
            out.insert(key, v);
        }
    }
    out
}

fn parse_sample(sample: &(serde_json::Value, String)) -> Option<f64> {
    sample.1.parse().ok()
}

#[derive(Debug, Deserialize)]
pub struct PromResponse {
    #[serde(default)]
    pub data: PromData,
}

#[derive(Debug, Default, Deserialize)]
pub struct PromData {
    #[serde(default)]
    pub result: Vec<PromResult>,
}

#[derive(Debug, Deserialize)]
pub struct PromResult {
    #[serde(default)]
    pub metric: HashMap<String, String>,
    pub value: Option<(serde_json::Value, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_instant_vector() {
        let body: PromResponse = serde_json::from_str(
            r#"{"data":{"result":[{"metric":{"path":"/v1/orders"},"value":[1,"0.42"]}]}}"#,
        )
        .unwrap();
        let v = parse_vector(&body);
        assert!((v["/v1/orders"] - 0.42).abs() < 1e-9);
    }

    #[test]
    fn sparkline_uses_block_chars() {
        let s = spark(&[0.0, 1.0, 0.5]);
        assert_eq!(s.chars().count(), 3);
        assert!(s.contains('▁') || s.contains('█') || s.contains('▄'));
    }
}
