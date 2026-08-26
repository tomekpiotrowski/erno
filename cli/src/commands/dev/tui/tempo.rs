use std::collections::HashMap;

use reqwest::Client;
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct TraceHit {
    pub trace_id: String,
    pub name: String,
    pub service: String,
    pub duration_ms: f64,
    pub start_unix_nano: String,
}

#[derive(Clone, Debug)]
pub struct Span {
    pub id: String,
    pub parent_id: String,
    pub name: String,
    pub service: String,
    pub duration_ms: f64,
    pub start_ms: f64,
    pub status: String,
    pub attributes: HashMap<String, String>,
    pub events: Vec<SpanEvent>,
    pub children: Vec<Span>,
}

#[derive(Clone, Debug)]
pub struct SpanEvent {
    pub name: String,
    pub attributes: HashMap<String, String>,
}

pub async fn search(
    client: &Client,
    base: &str,
    query: &str,
    window_secs: i64,
    limit: u32,
) -> Vec<TraceHit> {
    let end = chrono_now();
    let start = end - window_secs;
    let url = format!("{}/api/search", base.trim_end_matches('/'));
    let Ok(res) = client
        .get(url)
        .query(&[
            ("q", query),
            ("start", &start.to_string()),
            ("end", &end.to_string()),
            ("limit", &limit.to_string()),
        ])
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(body) = res.json::<SearchResponse>().await else {
        return Vec::new();
    };
    body.traces.into_iter().map(to_hit).collect()
}

pub async fn get_trace(client: &Client, base: &str, id: &str) -> Vec<Span> {
    let url = format!("{}/api/traces/{id}", base.trim_end_matches('/'));
    let Ok(res) = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(body) = res.json::<TraceResponse>().await else {
        return Vec::new();
    };
    to_tree(&body)
}

pub fn to_hit(row: SearchTrace) -> TraceHit {
    TraceHit {
        trace_id: row.trace_id.or(row.trace_id_alt).unwrap_or_default(),
        name: row.root_trace_name.unwrap_or_default(),
        service: row.root_service_name.unwrap_or_default(),
        duration_ms: row.duration_ms.unwrap_or(0.0),
        start_unix_nano: row.start_time_unix_nano.unwrap_or_default(),
    }
}

pub fn to_tree(res: &TraceResponse) -> Vec<Span> {
    let mut flat: Vec<Span> = Vec::new();
    for batch in &res.batches {
        let service = batch
            .resource
            .as_ref()
            .and_then(|r| r.attributes.iter().find(|a| a.key == "service.name"))
            .map(otel_string)
            .unwrap_or_default();
        let scopes = if !batch.scope_spans.is_empty() {
            &batch.scope_spans
        } else {
            &batch.instrumentation_library_spans
        };
        for scope in scopes {
            for span in &scope.spans {
                let start = nano_to_ms(&span.start_time_unix_nano);
                let end = nano_to_ms(&span.end_time_unix_nano);
                flat.push(Span {
                    id: hex(&span.span_id),
                    parent_id: hex(&span.parent_span_id),
                    name: span.name.clone().unwrap_or_else(|| "(span)".into()),
                    service: service.clone(),
                    duration_ms: (end - start).max(0.0),
                    start_ms: start,
                    status: status_label(&span.status),
                    attributes: span
                        .attributes
                        .iter()
                        .filter_map(|a| {
                            let v = otel_string(a);
                            (!v.is_empty()).then(|| (a.key.clone(), v))
                        })
                        .collect(),
                    events: span
                        .events
                        .iter()
                        .map(|e| SpanEvent {
                            name: e.name.clone().unwrap_or_default(),
                            attributes: e
                                .attributes
                                .iter()
                                .filter_map(|a| {
                                    let v = otel_string(a);
                                    (!v.is_empty()).then(|| (a.key.clone(), v))
                                })
                                .collect(),
                        })
                        .collect(),
                    children: Vec::new(),
                });
            }
        }
    }
    nest(flat)
}

fn nest(flat: Vec<Span>) -> Vec<Span> {
    let mut by_id: HashMap<String, Span> = HashMap::new();
    for s in flat {
        by_id.insert(s.id.clone(), s);
    }
    let ids: Vec<String> = by_id.keys().cloned().collect();
    let mut child_ids: HashMap<String, Vec<String>> = HashMap::new();
    let mut roots = Vec::new();
    for id in &ids {
        let parent = by_id
            .get(id)
            .map(|s| s.parent_id.clone())
            .unwrap_or_default();
        if !parent.is_empty() && by_id.contains_key(&parent) {
            child_ids.entry(parent).or_default().push(id.clone());
        } else {
            roots.push(id.clone());
        }
    }
    fn take(
        id: &str,
        by_id: &mut HashMap<String, Span>,
        child_ids: &HashMap<String, Vec<String>>,
    ) -> Option<Span> {
        let mut node = by_id.remove(id)?;
        if let Some(children) = child_ids.get(id) {
            node.children = children
                .iter()
                .filter_map(|c| take(c, by_id, child_ids))
                .collect();
            node.children
                .sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
        }
        Some(node)
    }
    let mut out: Vec<Span> = roots
        .iter()
        .filter_map(|id| take(id, &mut by_id, &child_ids))
        .collect();
    out.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
    out
}

pub fn flatten(spans: &[Span]) -> Vec<(usize, &Span)> {
    let mut out = Vec::new();
    fn walk<'a>(spans: &'a [Span], depth: usize, out: &mut Vec<(usize, &'a Span)>) {
        for s in spans {
            out.push((depth, s));
            walk(&s.children, depth + 1, out);
        }
    }
    walk(spans, 0, &mut out);
    out
}

pub fn n1_insight(spans: &[Span]) -> Option<String> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for (_, sp) in flatten(spans) {
        for ev in &sp.events {
            let sql = ev
                .attributes
                .get("db.statement")
                .cloned()
                .or_else(|| sqlish(&ev.name).then(|| ev.name.clone()));
            if let Some(sql) = sql {
                *counts.entry(normalize_sql(&sql)).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .filter(|(_, c)| *c >= 8)
        .map(|(sql, c)| format!("{c} similar queries · {sql}"))
}

fn sqlish(s: &str) -> bool {
    let u = s.trim_start().to_ascii_uppercase();
    u.starts_with("SELECT")
        || u.starts_with("INSERT")
        || u.starts_with("UPDATE")
        || u.starts_with("DELETE")
}

pub fn normalize_sql(sql: &str) -> String {
    let mut out = String::new();
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            out.push('?');
            for n in chars.by_ref() {
                if n == '\'' {
                    break;
                }
            }
        } else if c.is_ascii_digit() {
            out.push('?');
            while chars.peek().is_some_and(|n| n.is_ascii_digit()) {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn nano_to_ms(v: &Option<serde_json::Value>) -> f64 {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0) / 1e6,
        Some(serde_json::Value::String(s)) => s.parse::<f64>().unwrap_or(0.0) / 1e6,
        _ => 0.0,
    }
}

fn hex(id: &Option<String>) -> String {
    id.as_deref().unwrap_or("").to_ascii_lowercase()
}

fn status_label(status: &Option<OtelStatus>) -> String {
    match status.as_ref().and_then(|s| s.code.as_ref()) {
        Some(serde_json::Value::Number(n)) if n.as_u64() == Some(2) => "error".into(),
        Some(serde_json::Value::Number(n)) if n.as_u64() == Some(1) => "ok".into(),
        Some(serde_json::Value::String(s)) if s.contains("ERROR") || s == "2" => "error".into(),
        Some(serde_json::Value::String(s)) if s.contains("OK") || s == "1" => "ok".into(),
        _ => "unset".into(),
    }
}

fn otel_string(attr: &OtelAttribute) -> String {
    let Some(v) = &attr.value else {
        return String::new();
    };
    if let Some(s) = v.string_value.clone() {
        return s;
    }
    if let Some(n) = &v.int_value {
        return n.to_string();
    }
    if let Some(n) = v.double_value {
        return n.to_string();
    }
    if let Some(b) = v.bool_value {
        return b.to_string();
    }
    String::new()
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    #[serde(default)]
    traces: Vec<SearchTrace>,
}

#[derive(Debug, Deserialize)]
pub struct SearchTrace {
    #[serde(rename = "traceID")]
    trace_id: Option<String>,
    #[serde(rename = "traceId")]
    trace_id_alt: Option<String>,
    #[serde(rename = "rootServiceName")]
    root_service_name: Option<String>,
    #[serde(rename = "rootTraceName")]
    root_trace_name: Option<String>,
    #[serde(rename = "startTimeUnixNano")]
    start_time_unix_nano: Option<String>,
    #[serde(rename = "durationMs")]
    duration_ms: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct TraceResponse {
    #[serde(default)]
    batches: Vec<Batch>,
}

#[derive(Debug, Deserialize)]
struct Batch {
    resource: Option<Resource>,
    #[serde(default, rename = "scopeSpans")]
    scope_spans: Vec<ScopeSpans>,
    #[serde(default, rename = "instrumentationLibrarySpans")]
    instrumentation_library_spans: Vec<ScopeSpans>,
}

#[derive(Debug, Deserialize)]
struct Resource {
    #[serde(default)]
    attributes: Vec<OtelAttribute>,
}

#[derive(Debug, Deserialize)]
struct ScopeSpans {
    #[serde(default)]
    spans: Vec<OtelSpan>,
}

#[derive(Debug, Deserialize)]
struct OtelSpan {
    #[serde(rename = "spanId")]
    span_id: Option<String>,
    #[serde(rename = "parentSpanId")]
    parent_span_id: Option<String>,
    name: Option<String>,
    #[serde(rename = "startTimeUnixNano")]
    start_time_unix_nano: Option<serde_json::Value>,
    #[serde(rename = "endTimeUnixNano")]
    end_time_unix_nano: Option<serde_json::Value>,
    status: Option<OtelStatus>,
    #[serde(default)]
    attributes: Vec<OtelAttribute>,
    #[serde(default)]
    events: Vec<OtelEvent>,
}

#[derive(Debug, Deserialize)]
struct OtelStatus {
    code: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct OtelEvent {
    name: Option<String>,
    #[serde(default)]
    attributes: Vec<OtelAttribute>,
}

#[derive(Debug, Deserialize)]
struct OtelAttribute {
    key: String,
    value: Option<OtelValue>,
}

#[derive(Debug, Deserialize)]
struct OtelValue {
    #[serde(rename = "stringValue")]
    string_value: Option<String>,
    #[serde(rename = "intValue")]
    int_value: Option<serde_json::Value>,
    #[serde(rename = "doubleValue")]
    double_value: Option<f64>,
    #[serde(rename = "boolValue")]
    bool_value: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_spans_and_keeps_events() {
        let body: TraceResponse = serde_json::from_str(
            r#"{
              "batches":[{
                "resource":{"attributes":[{"key":"service.name","value":{"stringValue":"erno"}}]},
                "scopeSpans":[{"spans":[
                  {"spanId":"aa","name":"GET /x","startTimeUnixNano":"1000000000","endTimeUnixNano":"2000000000","status":{"code":1}},
                  {"spanId":"bb","parentSpanId":"aa","name":"db","startTimeUnixNano":"1100000000","endTimeUnixNano":"1500000000",
                   "events":[{"name":"query","attributes":[{"key":"db.statement","value":{"stringValue":"SELECT 1"}}]}]}
                ]}]
              }]
            }"#,
        )
        .unwrap();
        let tree = to_tree(&body);
        assert_eq!(tree[0].name, "GET /x");
        assert_eq!(
            tree[0].children[0].events[0].attributes["db.statement"],
            "SELECT 1"
        );
    }

    #[test]
    fn n1_fires_at_eight_similar_selects() {
        let ev = SpanEvent {
            name: "q".into(),
            attributes: HashMap::from([(
                "db.statement".into(),
                "SELECT * FROM t WHERE id=1".into(),
            )]),
        };
        let child = Span {
            id: "c".into(),
            parent_id: "r".into(),
            name: "sql".into(),
            service: "api".into(),
            duration_ms: 1.0,
            start_ms: 0.0,
            status: "ok".into(),
            attributes: HashMap::new(),
            events: vec![ev; 8],
            children: vec![],
        };
        let root = Span {
            id: "r".into(),
            parent_id: String::new(),
            name: "GET /".into(),
            service: "api".into(),
            duration_ms: 10.0,
            start_ms: 0.0,
            status: "ok".into(),
            attributes: HashMap::new(),
            events: vec![],
            children: vec![child],
        };
        let msg = n1_insight(&[root]).expect("insight");
        assert!(msg.contains("8 similar"), "{msg}");
        assert!(msg.contains("SELECT"), "{msg}");
    }

    #[test]
    fn normalize_strips_literals() {
        assert_eq!(
            normalize_sql("SELECT * FROM t WHERE id = 12 AND name = 'x'"),
            "SELECT * FROM t WHERE id = ? AND name = ?"
        );
    }
}
