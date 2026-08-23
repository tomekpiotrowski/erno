//! Alerting on signals the collector already holds.
//!
//! Docs: docs/src/content/docs/monitoring/alerts.md
//!
//! Built rather than bundled. Alertmanager only sees Prometheus metrics, so it
//! structurally cannot express "a new error type appeared" or "this issue is
//! spiking" — which are the alerts most worth having here. Bundling it would
//! mean running two alerting systems with two config languages and two
//! notification setups, and still writing the error half by hand.
//!
//! Rules read from three sources today: the error store, uptime checks, and
//! application health readings. A PromQL source is the natural fourth and is
//! left as an extension point rather than half-built.

pub mod evaluator;
pub mod notifier;
pub mod rules;
pub mod runner;
pub mod service;

pub use rules::{
    advance, Comparator, Notify, RuleSource, RuleState, RuleStatus, RuleTiming, Transition,
};
pub use runner::spawn as spawn_alerting;
