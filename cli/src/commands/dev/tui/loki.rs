use reqwest::Client;
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct LokiLine {
    pub ts_ms: f64,
    pub line: String,
    pub service: String,
}

pub fn build_logql(trace_id: &str) -> String {
    format!(
        "{{service_name=~\".+\"}} | trace_id=\"{}\"",
        escape(trace_id)
    )
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub async fn range(client: &Client, base: &str, query: &str, window_secs: i64) -> Vec<LokiLine> {
    let end_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    let start_ns = end_ns - window_secs * 1_000_000_000;
    let url = format!("{}/loki/api/v1/query_range", base.trim_end_matches('/'));
    let Ok(res) = client
        .get(url)
        .query(&[
            ("query", query),
            ("start", &start_ns.to_string()),
            ("end", &end_ns.to_string()),
            ("limit", "200"),
            ("direction", "backward"),
        ])
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(body) = res.json::<LokiResponse>().await else {
        return Vec::new();
    };
    flatten(&body)
}

pub fn flatten(res: &LokiResponse) -> Vec<LokiLine> {
    let mut lines = Vec::new();
    for stream in &res.data.result {
        let service = stream
            .stream
            .get("service_name")
            .cloned()
            .unwrap_or_default();
        for (ts, line) in &stream.values {
            let ts_ms = ts.parse::<f64>().unwrap_or(0.0) / 1e6;
            lines.push(LokiLine {
                ts_ms,
                line: line.clone(),
                service: service.clone(),
            });
        }
    }
    lines.sort_by(|a, b| b.ts_ms.total_cmp(&a.ts_ms));
    lines
}

pub fn panic_frames(lines: &[LokiLine]) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for l in lines {
        if let Some(frame) = parse_at_frame(&l.line) {
            out.push(frame);
        }
        if let Some(frame) = parse_panicked_at(&l.line) {
            out.push(frame);
        }
    }
    out
}

fn parse_panicked_at(line: &str) -> Option<(String, u32)> {
    let rest = line.split("panicked at ").nth(1)?;
    let loc = rest.split([' ', ':']).take(2).collect::<Vec<_>>();
    // "file.rs:12:" or "file.rs:12:1"
    let file = rest.split(':').next()?.trim();
    if !file.contains('.') {
        return None;
    }
    let line_no = rest
        .split(':')
        .nth(1)?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    let n = line_no.parse().ok()?;
    let _ = loc;
    Some((file.to_string(), n))
}

fn parse_at_frame(line: &str) -> Option<(String, u32)> {
    let rest = line.trim().strip_prefix("at ")?;
    let (file, line_no) = rest.rsplit_once(':')?;
    Some((file.to_string(), line_no.parse().ok()?))
}

#[derive(Debug, Default, Deserialize)]
pub struct LokiResponse {
    #[serde(default)]
    pub data: LokiData,
}

#[derive(Debug, Default, Deserialize)]
pub struct LokiData {
    #[serde(default)]
    pub result: Vec<LokiStream>,
}

#[derive(Debug, Deserialize)]
pub struct LokiStream {
    #[serde(default)]
    pub stream: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub values: Vec<(String, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_streams_newest_first() {
        let body: LokiResponse = serde_json::from_str(
            r#"{"data":{"result":[{"stream":{"service_name":"erno"},"values":[["2000000000","b"],["1000000000","a"]]}]}}"#,
        )
        .unwrap();
        let lines = flatten(&body);
        assert_eq!(lines[0].line, "b");
        assert_eq!(lines[1].line, "a");
        assert_eq!(lines[0].service, "erno");
    }

    #[test]
    fn parses_panic_location() {
        let (f, n) = parse_panicked_at("thread panicked at src/orders.rs:118:5:").unwrap();
        assert_eq!(f, "src/orders.rs");
        assert_eq!(n, 118);
    }

    #[test]
    fn logql_escapes_quotes() {
        assert!(build_logql(r#"ab"c"#).contains(r#"ab\"c"#));
    }
}
