//! Coordinated graceful shutdown.
//!
//! Kubernetes stops a pod by sending `SIGTERM` and waiting
//! `terminationGracePeriodSeconds` before `SIGKILL`. Without a handler the
//! process ignores the signal, sits out the whole grace period, and is then
//! killed outright — so every rolling deploy cut in-flight requests, dropped
//! whatever the error reporter had buffered, and, worst of all, killed job
//! workers mid-job and left their rows stuck in `running` until the stuck-job
//! sweeper reclaimed them.
//!
//! One [`Shutdown`] is created at boot and cloned to everything long-running.
//! Each holder either selects on [`Shutdown::recv`] in its loop or hands the
//! future to `axum::serve(..).with_graceful_shutdown(..)`.
//!
//! Docs: docs/src/content/docs/api/boot.md

use std::time::Duration;

use tokio::sync::watch;
use tracing::info;

/// How long to wait for background tasks after the signal.
///
/// Deliberately shorter than a typical 30s `terminationGracePeriodSeconds`, so
/// the process exits on its own terms rather than being `SIGKILL`ed partway
/// through the tidying it is trying to do.
pub const DRAIN_TIMEOUT: Duration = Duration::from_secs(25);

/// A clonable handle that resolves once the process should stop.
#[derive(Clone, Debug)]
pub struct Shutdown(watch::Receiver<bool>);

/// The sending half. Held by the boot path; dropping it does not trigger
/// shutdown, so a task holding only a [`Shutdown`] cannot be woken by accident.
#[derive(Clone, Debug)]
pub struct ShutdownSignal(watch::Sender<bool>);

impl ShutdownSignal {
    /// Tell every holder to stop. Idempotent.
    pub fn trigger(&self) {
        let _ = self.0.send(true);
    }
}

/// Start listening for `SIGTERM` and Ctrl-C.
///
/// Returns the trigger and a handle to clone into long-running tasks.
#[must_use]
pub fn listen() -> (ShutdownSignal, Shutdown) {
    let (tx, rx) = watch::channel(false);
    let signal = ShutdownSignal(tx);

    let spawned = signal.clone();
    tokio::spawn(async move {
        wait_for_signal().await;
        info!("🛑 Shutdown signal received; draining");
        spawned.trigger();
    });

    (signal, Shutdown(rx))
}

/// A handle that never fires. For tests and for embedding contexts that manage
/// their own lifecycle.
#[must_use]
pub fn never() -> Shutdown {
    let (tx, rx) = watch::channel(false);
    // Keep the sender alive for the process lifetime: a closed channel would
    // make `recv` return immediately, which is the opposite of "never".
    std::mem::forget(tx);
    Shutdown(rx)
}

impl Shutdown {
    /// A handle that never fires — for non-serving paths (tests, one-shot
    /// commands) that build an [`crate::app::App`] without a signal listener.
    #[must_use]
    pub fn never() -> Self {
        let (tx, rx) = watch::channel(false);
        // Leak the sender so the channel never closes; a leaked watch sender
        // is one allocation for the life of the process.
        std::mem::forget(tx);
        Self(rx)
    }

    /// Whether shutdown has already been requested.
    #[must_use]
    pub fn is_triggered(&self) -> bool {
        *self.0.borrow()
    }

    /// Resolve when shutdown has been requested.
    ///
    /// Returns immediately if it already has, so a task started late does not
    /// miss the signal.
    pub async fn recv(&mut self) {
        if *self.0.borrow() {
            return;
        }
        // Only errors when the sender is gone, which means nothing will ever
        // trigger — treat that as "keep running" rather than as shutdown.
        let _ = self.0.changed().await;
    }

    /// Whether shutdown has been requested, without waiting.
    #[must_use]
    pub fn is_shutting_down(&self) -> bool {
        *self.0.borrow()
    }
}

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("could not install a SIGTERM handler: {e}");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    tokio::select! {
        // What Kubernetes actually sends.
        _ = terminate.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_handle_resolves_once_triggered() {
        let (signal, mut shutdown) = {
            let (tx, rx) = watch::channel(false);
            (ShutdownSignal(tx), Shutdown(rx))
        };
        assert!(!shutdown.is_shutting_down());
        signal.trigger();
        assert!(shutdown.is_shutting_down());
        // Must not hang: the state is already set.
        shutdown.recv().await;
    }

    #[tokio::test]
    async fn a_handle_created_before_the_signal_still_sees_it() {
        let (tx, rx) = watch::channel(false);
        let signal = ShutdownSignal(tx);
        let mut shutdown = Shutdown(rx);
        tokio::spawn(async move {
            signal.trigger();
            // Hold the sender so the channel does not close first, which would
            // make this pass for the wrong reason.
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        shutdown.recv().await;
        assert!(shutdown.is_shutting_down());
    }

    #[tokio::test]
    async fn triggering_twice_is_harmless() {
        let (tx, rx) = watch::channel(false);
        let signal = ShutdownSignal(tx);
        signal.trigger();
        signal.trigger();
        assert!(Shutdown(rx).is_shutting_down());
    }

    #[tokio::test]
    async fn a_never_handle_does_not_resolve() {
        let mut shutdown = never();
        assert!(!shutdown.is_shutting_down());
        let waited = tokio::time::timeout(Duration::from_millis(50), shutdown.recv()).await;
        assert!(waited.is_err(), "never() must not resolve");
    }
}
