//! The `erno-monitoring` binary.
//!
//! Deliberately thin: everything is in the library beside it, so the collector
//! can be depended on and tested as a library rather than only run as a
//! process.

#[tokio::main]
async fn main() {
    erno_monitoring::boot().await;
}
