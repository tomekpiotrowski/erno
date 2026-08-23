//! Router state for the collector.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md

use std::sync::Arc;

use crate::{app::App, error_reporting::config::CollectorConfig};

use super::ingest::CollectorSink;

/// What collector handlers are given.
///
/// A composite rather than plain [`App`] because handlers need the sink and the
/// collector configuration alongside the database. Operator routes therefore
/// cannot use the [`crate::admin::auth::AdminAuth`] extractor, which is bound to
/// `App` as its state type; they call
/// [`crate::admin::auth::verify_admin_basic_auth`] through a middleware instead,
/// so there is still only one implementation of the check.
pub struct CollectorState<ExtraConfig: Clone + Send + Sync + 'static> {
    /// The host application: database, mailer, rate limiter, config.
    pub app: App<ExtraConfig>,
    /// Collector settings.
    pub config: Arc<CollectorConfig>,
    /// Where accepted reports go.
    pub sink: CollectorSink,
}

// Derived `Clone` would demand `ExtraConfig: Clone` on the struct itself, which
// then leaks into every handler bound. Implementing it by hand keeps the bound
// where it belongs.
impl<ExtraConfig: Clone + Send + Sync + 'static> Clone for CollectorState<ExtraConfig> {
    fn clone(&self) -> Self {
        Self {
            app: self.app.clone(),
            config: Arc::clone(&self.config),
            sink: self.sink.clone(),
        }
    }
}
