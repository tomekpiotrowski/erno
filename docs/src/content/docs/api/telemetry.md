---
title: Telemetry
description: Distributed tracing with tracing-subscriber and Prometheus metrics
sidebar:
  order: 8
---

> **Source**: `api/src/metrics/`

Erno configures both structured logging/tracing and Prometheus metrics automatically on startup. No manual setup is required.

## Tracing

Erno uses [`tracing`](https://crates.io/crates/tracing) and [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber) for structured, leveled logging.

### Log level configuration

```toml
[tracing]
log_level = "info"   # applied when running in server mode
```

Override at runtime with the standard `RUST_LOG` environment variable:

```bash
RUST_LOG=debug cargo run -- serve
```

CLI subcommands (`migrate`, `routes`, etc.) automatically reduce log level to `warn` or `error` to keep output clean.

### Log format

Logs are emitted in compact, human-readable format with timestamps, level, and message. Module paths and thread info are suppressed for readability.

## Prometheus metrics

Erno exposes a `/metrics` endpoint (configurable path) in Prometheus text format.

### Configuration

```toml
[metrics]
enabled = true
path = "/metrics"            # default
# auth_token = "secret"      # require Bearer token to scrape
db_stats_interval_seconds = 30
table_counts = ["users", "jobs"]   # report row counts for these tables
```

### Built-in metrics

| Metric | Type | Description |
|--------|------|-------------|
| `http_requests_total` | Counter | Total requests, labeled by method, path, status |
| `http_request_duration_seconds` | Histogram | Request latency distribution |
| `http_requests_in_flight` | Gauge | Currently active requests |
| `db_pool_*` | Gauge | Connection pool stats (size, idle, available) |

Database table row counts are reported as `db_table_row_count{table="..."}` gauges when `table_counts` is configured.

### Securing the endpoint

Set `auth_token` to require a Bearer token when scraping:

```toml
[metrics]
auth_token = "your-scrape-token"
```

Your Prometheus scrape config:

```yaml
scrape_configs:
  - job_name: myapp
    static_configs:
      - targets: ['myapp:3000']
    bearer_token: your-scrape-token
```

### Custom metrics

Use the [`metrics`](https://crates.io/crates/metrics) crate directly — the Prometheus recorder is installed globally by Erno on startup:

```rust
use metrics::{counter, histogram, gauge};

counter!("my_event_total", "type" => "signup").increment(1);
histogram!("my_operation_seconds").record(elapsed.as_secs_f64());
gauge!("queue_depth").set(queue.len() as f64);
```
