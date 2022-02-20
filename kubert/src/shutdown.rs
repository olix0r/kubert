//! Drives graceful shutdown when the process receives a signal.

use tokio::signal::unix::{signal, SignalKind};
use tracing::debug;

#[cfg_attr(docsrs, doc(cfg(any(feature = "shutdown"))))]
pub use drain::Watch;

/// Drives shutdown by watching signals
#[derive(Debug)]
#[must_use = "call `Shutdown::on_signal` to await a signal"]
#[cfg_attr(docsrs, doc(cfg(any(feature = "shutdown"))))]
pub struct Shutdown(drain::Signal);

/// Indicates whether shutdown completed gracefully or was forced by a second signal
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(docsrs, doc(cfg(any(feature = "shutdown"))))]
pub enum Completion {
    /// Indicates that shutdown completed gracefully
    Graceful,

    /// Indicates that shutdown did not complete gracefully
    Aborted,
}

/// Creates a shutdown channel
///
/// [`Shutdown`] watches for `SIGINT` and `SIGTERM` signals. When a signal is received, [`Watch`]
/// instances are notifed and, when all watches are dropped, the shutdown is completed. If a second
/// signal is received while waiting for watches to be dropped, the shutdown is aborted.
///
/// If a second signal is received while waiting for shutdown to complete, the process
#[cfg_attr(docsrs, doc(cfg(any(feature = "shutdown"))))]
pub fn channel() -> (Shutdown, Watch) {
    let (drain_tx, drain_rx) = drain::channel();
    (Shutdown(drain_tx), drain_rx)
}

impl Shutdown {
    /// Watches for signals and drives shutdown
    ///
    /// When a `SIGINT` or `SIGTERM` signal is received, the shutdown is initiated, notifying all
    /// [`Watch`] instances. When all watches are dropped, the shutdown is completed.
    ///
    /// If a second signal is received while waiting for watches to be dropped, this future
    /// completes immediately and [`Completion::Aborted`] is returned.
    ///
    /// An error is returned when signal registration fails.
    pub async fn on_signal(self) -> std::io::Result<Completion> {
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;

        tokio::select! {
            _ = interrupt.recv() => {
                debug!("Received SIGINT; draining");
            },

            _ = terminate.recv() => {
                debug!("Received SIGTERM; draining");
            }
        }

        tokio::select! {
            _ = self.0.drain() => {
                debug!("Drained");
                Ok(Completion::Graceful)
            },

            _ = interrupt.recv() => {
                debug!("Received SIGINT; aborting");
                Ok(Completion::Aborted)
            },

            _ = terminate.recv() => {
                debug!("Received SIGTERM; aborting");
                Ok(Completion::Aborted)
            }
        }
    }
}

impl Completion {
    /// Returns `true` if the shutdown completed gracefully
    pub fn is_graceful(&self) -> bool {
        matches!(self, Completion::Graceful)
    }

    /// Returns `true` if the shutdown was aborted
    pub fn is_aborted(&self) -> bool {
        matches!(self, Completion::Aborted)
    }
}
