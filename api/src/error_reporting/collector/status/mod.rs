//! Public status page.
//!
//! Docs: docs/src/content/docs/monitoring/status-page.md
//!
//! The defining constraint: **the page must not depend on this service being
//! up**. A status page that goes down during an outage is worse than none,
//! because readers conclude nothing is wrong.
//!
//! So the collector does not serve the page. It periodically *publishes* a
//! static JSON document to object storage, and the page — hosted separately,
//! ideally on a CDN — reads only that. If the document goes stale the page says
//! so rather than showing a reassuring green.

pub mod publisher;
pub mod service;
pub mod snapshot;

pub use publisher::spawn as spawn_publisher;
pub use snapshot::{PublicState, StatusSnapshot};
