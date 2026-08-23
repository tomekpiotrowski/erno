# Erno Monitoring

Separate deployment that watches an Erno application: errors, deploys,
subsystem health, uptime checks, alerts and a public status page. Its own
binary, its own database, its own operator console — deployed to infrastructure
separate from the application, so it survives the outages it exists to report.

All commands below run from this directory (`monitoring/`).

## Building & testing

```sh
cargo build
cargo test                  # requires PostgreSQL — see below
cargo clippy -- -D warnings
cargo fmt

cd ui && npm start          # operator console on :4400
cd ui && npm run build
```

**Tests require PostgreSQL** at `postgres://erno:erno@localhost/erno_monitoring_test`
(configured in `config/test.toml`). This is a *different* database from the
API's test database; both suites reset their own schema, so sharing one would
have them destroy each other's data.

**Tests run single-threaded** (`.cargo/config.toml` sets `RUST_TEST_THREADS=1`).
Several tests operate table-wide by nature — the retention sweep, the regression
tests that resolve every issue — and in parallel those block on rows other tests
have inserted but not yet rolled back, which Postgres reports as a deadlock.

Run the collector locally with `cargo run -- serve` (port 3001, database
`erno_monitoring`). Apply migrations with `cargo run -- db migrate up`.

## Layout

| Path | What it is |
|------|------------|
| `src/main.rs` | Boots an ordinary Erno app and mounts the collector |
| `src/config.rs` | `MonitorConfig` — the `[collector]` section, via Erno's `ExtraConfig` |
| `src/migrator.rs` | Framework migrations chained ahead of the collector's |
| `src/tests.rs` | Collector request tests |
| `status/` | The public status page: one dependency-free HTML file |
| `ui/` | Angular operator console, scaffolded from `admin/` |

The collector's own logic lives in the library, at
`api/src/error_reporting/collector/` — this crate is a thin consumer. Subsystem
health gathering lives in `api/src/health/`, because the application side needs
it too.

| Collector module | What it does |
|---|---|
| `ingest`, `fingerprint`, `scrub` | Error ingest, grouping and redaction |
| `releases` | Deploy tracking |
| `health` | Application heartbeats and their verdicts |
| `uptime` | Synthetic probes and flap damping |
| `alerting` | Rule evaluation, state machine, notifications |
| `status` | Public snapshot and its publisher |
| `retention` | Bounding what is kept |

## Architecture notes

- **An Erno application**: config, migrations, jobs, mailer, metrics, health
  checks, and operator Basic auth all come from the library.
- **Two ingest credentials**: a trusted server token and a *public* browser
  token. The browser token ships in JS bundles and is a speed bump, not a
  security control.
- **Collector migrations are not in `erno_migrations()`** — they belong to this
  database, and adding them to the framework list would give every application
  deployment tables it never writes to.
- **Test isolation**: `config/test.toml` sets `sync_writes = true` so ingest
  writes on the request's own connection and rolls back with the test. A
  background writer on a second connection would deadlock the single-connection
  test pool and never see the test's data.
- **Operator auth is independent of the application's auth service**, which may
  be exactly what is broken when an operator needs this console.
- **Singleton background work takes an advisory lock**: retention, the uptime
  prober, the status publisher and the alert evaluator each hold one, so
  replicas do not duplicate probes or multiply every alert by the replica count.
- **Issue upserts are sorted by fingerprint** before the multi-row statement, so
  concurrent writers take index locks in the same order. Without it, two batches
  sharing fingerprints in different orders deadlock.
- **The status page must not depend on this service.** The collector publishes a
  static document; the page reads only that.

## Documentation

| Topic | Doc page |
|-------|----------|
| Deployment overview | `docs/src/content/docs/monitoring/index.md` |
| Error reporting | `docs/src/content/docs/monitoring/error-reporting.md` |
| Releases | `docs/src/content/docs/monitoring/releases.md` |
| Subsystem health | `docs/src/content/docs/monitoring/subsystem-health.md` |
| Uptime checks | `docs/src/content/docs/monitoring/uptime.md` |
| Alerts | `docs/src/content/docs/monitoring/alerts.md` |
| Status page | `docs/src/content/docs/monitoring/status-page.md` |
| Angular SDK | `docs/src/content/docs/app/error-reporting.md` |

**If you change the ingest contract, the config keys, or the grouping rules,
update the corresponding doc page.**
