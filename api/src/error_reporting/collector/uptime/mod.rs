//! Synthetic uptime checks.
//!
//! Docs: docs/src/content/docs/monitoring/uptime.md
//!
//! Without these, a status page is a manual claim. With them it is an
//! observation.
//!
//! One honest caveat, documented rather than hidden: probes run *from the
//! monitoring deployment*, so they verify the application from outside the
//! application but not from outside the monitoring provider. A network fault on
//! the monitoring side reads as an application outage.

pub mod probe;
pub mod runner;
pub mod service;
pub mod state;

pub use runner::spawn as spawn_runner;
pub use state::{apply_probe, CheckState, ProbeOutcome, StateTransition};
