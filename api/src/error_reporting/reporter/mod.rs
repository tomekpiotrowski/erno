//! The sending half of error reporting.
//!
//! Docs: docs/src/content/docs/monitoring/error-reporting.md
//!
//! Held by every Erno application. Because the collector now lives on separate
//! infrastructure, reporting crosses the network — so the governing constraint
//! is that **the reporter must never be able to hurt the application it
//! reports for**. Everything here follows from that:
//!
//! * [`ErrorReporter::capture`] is synchronous, non-blocking and infallible, so
//!   a `tracing` layer and a panic hook (which cannot await, and may not even be
//!   on the runtime) can call it.
//! * The queue is bounded and sheds the newest reports when full.
//! * The sender applies a timeout, exponential backoff, and a circuit breaker.
//! * Failures are never reported through the reporter.

pub mod backoff;
pub mod capture;
pub mod circuit_breaker;
pub mod handle;
pub mod sender;

#[cfg(test)]
mod tests;

pub use circuit_breaker::CircuitBreaker;
pub use handle::ErrorReporter;
